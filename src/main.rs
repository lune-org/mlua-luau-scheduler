use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use gxhash::GxHashMap;
use mlua::prelude::*;
use tokio::{
    runtime::Runtime as TokioRuntime,
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::{spawn_blocking, LocalSet},
    time::{sleep, Instant},
};

mod thread_id;
use thread_id::ThreadId;

const NUM_TEST_BATCHES: usize = 20;
const NUM_TEST_THREADS: usize = 50_000;

const MAIN_CHUNK: &str = r#"
wait(0.001 * math.random())
"#;

const WAIT_IMPL: &str = r#"
__scheduler__resumeAfter(...)
coroutine.yield()
"#;

type ThreadMap<'lua> = GxHashMap<ThreadId, LuaThread<'lua>>;

type MessageSender = UnboundedSender<Message>;
type MessageReceiver = UnboundedReceiver<Message>;

enum Message {
    Resume(ThreadId),
    Cancel(ThreadId),
    Sleep(ThreadId, Duration),
    Error(ThreadId, LuaError),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum StatsCounter {
    Resumed,
    Cancelled,
    Slept,
    Errored,
}

#[derive(Debug, Clone)]
struct Stats {
    start: Instant,
    counters: Arc<DashMap<StatsCounter, usize>>,
}

impl Stats {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            counters: Arc::new(DashMap::new()),
        }
    }

    fn incr(&self, counter: StatsCounter) {
        self.counters
            .entry(counter)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    fn elapsed(&self) -> Duration {
        Instant::now() - self.start
    }
}

fn main() {
    let rt = TokioRuntime::new().unwrap();
    let set = LocalSet::new();
    let _guard = set.enter();

    let (msg_tx, lua_rx) = unbounded_channel::<Message>();
    let (lua_tx, msg_rx) = unbounded_channel::<Message>();

    let stats = Stats::new();
    let stats_inner = stats.clone();

    set.block_on(&rt, async {
        let res = select! {
            r = spawn(main_async_task(msg_rx, msg_tx, stats_inner.clone())) => r,
            r = spawn_blocking(|| main_lua_task(lua_rx, lua_tx, stats_inner)) => r,
        };
        if let Err(e) = res {
            eprintln!("Runtime fatal error: {e}");
        }
    });

    println!("Finished running in {:?}", stats.elapsed());
    println!("Thread counters: {:#?}", stats.counters);
}

fn main_lua_task(mut rx: MessageReceiver, tx: MessageSender, stats: Stats) -> LuaResult<()> {
    let lua = Lua::new();
    let g = lua.globals();

    lua.enable_jit(true);
    lua.set_app_data(tx.clone());

    let send_message = |lua: &Lua, msg: Message| {
        lua.app_data_ref::<MessageSender>()
            .unwrap()
            .send(msg)
            .unwrap();
    };

    g.set(
        "__scheduler__resumeAfter",
        LuaFunction::wrap(move |lua, duration: f64| {
            let thread_id = ThreadId::from(lua.current_thread());
            let duration = Duration::from_secs_f64(duration);
            send_message(lua, Message::Sleep(thread_id, duration));
            Ok(())
        }),
    )?;

    g.set(
        "__scheduler__cancel",
        LuaFunction::wrap(move |lua, thread: LuaThread| {
            let thread_id = ThreadId::from(thread);
            send_message(lua, Message::Cancel(thread_id));
            Ok(())
        }),
    )?;

    g.set("wait", lua.load(WAIT_IMPL).into_function()?)?;

    let mut yielded_threads = ThreadMap::default();
    let mut runnable_threads = ThreadMap::default();

    println!("Running {NUM_TEST_BATCHES} batches");
    for _ in 0..NUM_TEST_BATCHES {
        let main_fn = lua.load(MAIN_CHUNK).into_function()?;
        for _ in 0..NUM_TEST_THREADS {
            let thread = lua.create_thread(main_fn.clone())?;
            runnable_threads.insert(ThreadId::from(&thread), thread);
        }

        loop {
            // Runnable / yielded threads may be empty because of cancellation
            if runnable_threads.is_empty() && yielded_threads.is_empty() {
                break;
            }

            // Resume as many threads as possible
            for (thread_id, thread) in runnable_threads.drain() {
                stats.incr(StatsCounter::Resumed);
                if let Err(e) = thread.resume::<_, ()>(()) {
                    stats.incr(StatsCounter::Errored);
                    send_message(&lua, Message::Error(thread_id, e));
                }
                if thread.status() == LuaThreadStatus::Resumable {
                    yielded_threads.insert(thread_id, thread);
                }
            }

            if yielded_threads.is_empty() {
                break; // All threads ran, and we don't have any async task that can spawn more
            }

            // Wait for at least one message, but try to receive as many as possible
            let mut process_message = |message| match message {
                Message::Resume(thread_id) => {
                    if let Some(thread) = yielded_threads.remove(&thread_id) {
                        runnable_threads.insert(thread_id, thread);
                    }
                }
                Message::Cancel(thread_id) => {
                    yielded_threads.remove(&thread_id);
                    runnable_threads.remove(&thread_id);
                    stats.incr(StatsCounter::Cancelled);
                }
                _ => unreachable!(),
            };
            if let Some(message) = rx.blocking_recv() {
                process_message(message);
                while let Ok(message) = rx.try_recv() {
                    process_message(message);
                }
            } else {
                break; // Scheduler exited
            }
        }
    }

    Ok(())
}

async fn main_async_task(
    mut rx: MessageReceiver,
    tx: MessageSender,
    stats: Stats,
) -> LuaResult<()> {
    // Set up message processor
    let process_message = |message| match message {
        Message::Sleep(thread_id, duration) => {
            stats.incr(StatsCounter::Slept);
            let tx = tx.clone();
            spawn(async move {
                sleep(duration).await;
                let _ = tx.send(Message::Resume(thread_id));
            });
        }
        Message::Error(_, e) => {
            eprintln!("Lua error: {e}");
        }
        _ => unreachable!(),
    };

    // Wait for at least one message, but try to receive as many as possible
    while let Some(message) = rx.recv().await {
        process_message(message);
        while let Ok(message) = rx.try_recv() {
            process_message(message);
        }
    }

    Ok(())
}
