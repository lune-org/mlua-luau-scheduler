use std::{collections::HashMap, time::Duration};

use mlua::prelude::*;

mod thread_id;
use thread_id::ThreadId;
use tokio::{
    runtime::Runtime as TokioRuntime,
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::LocalSet,
    time::{interval, sleep, Instant, MissedTickBehavior},
};

const NUM_TEST_BATCHES: usize = 20;
const NUM_TEST_THREADS: usize = 50_000;

const MAIN_CHUNK: &str = r#"
wait(0.001 * math.random())
"#;

const WAIT_IMPL: &str = r#"
__scheduler__resumeAfter(...)
coroutine.yield()
"#;

type RuntimeSender = UnboundedSender<RuntimeMessage>;
type RuntimeReceiver = UnboundedReceiver<RuntimeMessage>;

#[derive(Debug, Clone, Copy)]
enum RuntimeMessage {
    Resume(ThreadId),
    Cancel(ThreadId),
    Yield(ThreadId, Duration),
}

fn main() {
    let rt = TokioRuntime::new().unwrap();
    let set = LocalSet::new();
    let _guard = set.enter();

    let (msg_tx, lua_rx) = unbounded_channel::<RuntimeMessage>();
    let (lua_tx, msg_rx) = unbounded_channel::<RuntimeMessage>();

    set.block_on(&rt, async {
        select! {
            _ = set.spawn_local(lua_main(lua_rx, lua_tx)) => {},
            _ = spawn(sched_main(msg_rx, msg_tx)) => {},
        }
    });
}

async fn lua_main(mut rx: RuntimeReceiver, tx: RuntimeSender) -> LuaResult<()> {
    let lua = Lua::new();
    let g = lua.globals();

    lua.enable_jit(true);
    lua.set_app_data(tx.clone());

    let send_message = |lua: &Lua, msg: RuntimeMessage| {
        lua.app_data_ref::<RuntimeSender>()
            .unwrap()
            .send(msg)
            .unwrap();
    };

    g.set(
        "__scheduler__resumeAfter",
        LuaFunction::wrap(move |lua, duration: f64| {
            let thread_id = ThreadId::from(lua.current_thread());
            let duration = Duration::from_secs_f64(duration);
            send_message(lua, RuntimeMessage::Yield(thread_id, duration));
            Ok(())
        }),
    )?;

    g.set(
        "__scheduler__cancel",
        LuaFunction::wrap(move |lua, thread: LuaThread| {
            let thread_id = ThreadId::from(thread);
            send_message(lua, RuntimeMessage::Cancel(thread_id));
            Ok(())
        }),
    )?;

    g.set("wait", lua.load(WAIT_IMPL).into_function()?)?;

    let mut yielded_threads: HashMap<ThreadId, LuaThread> = HashMap::new();
    let mut runnable_threads: HashMap<ThreadId, LuaThread> = HashMap::new();

    let before = Instant::now();

    let mut throttle = interval(Duration::from_millis(5));
    throttle.set_missed_tick_behavior(MissedTickBehavior::Delay);

    for n in 1..=NUM_TEST_BATCHES {
        println!("Running batch {n} of {NUM_TEST_BATCHES}");

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

            // Limit this loop to a maximum of 200hz, this lets us improve performance
            // by batching more work and not switching between running threads and waiting
            // for the next message as often. It may however add another 5 milliseconds of
            // latency to something like a web server, but the tradeoff is worth it.
            throttle.tick().await;

            // Resume as many threads as possible
            for (thread_id, thread) in runnable_threads.drain() {
                thread.resume(())?;
                if thread.status() == LuaThreadStatus::Resumable {
                    yielded_threads.insert(thread_id, thread);
                }
            }

            if yielded_threads.is_empty() {
                break; // All threads ran and we don't have any async task that can spawn more
            }

            // Wait for at least one message, but try to receive as many as possible
            let mut process_message = |message| match message {
                RuntimeMessage::Resume(thread_id) => {
                    if let Some(thread) = yielded_threads.remove(&thread_id) {
                        runnable_threads.insert(thread_id, thread);
                    }
                }
                RuntimeMessage::Cancel(thread_id) => {
                    yielded_threads.remove(&thread_id);
                    runnable_threads.remove(&thread_id);
                }
                _ => unreachable!(),
            };
            if let Some(message) = rx.recv().await {
                process_message(message);
                while let Ok(message) = rx.try_recv() {
                    process_message(message);
                }
            } else {
                break; // Scheduler exited
            }
        }
    }

    let after = Instant::now();
    println!(
        "Ran {} threads in {:?}",
        NUM_TEST_BATCHES * NUM_TEST_THREADS,
        after - before
    );

    Ok(())
}

async fn sched_main(mut rx: RuntimeReceiver, tx: RuntimeSender) -> LuaResult<()> {
    while let Some(message) = rx.recv().await {
        match message {
            RuntimeMessage::Yield(thread_id, duration) => {
                let tx = tx.clone();
                spawn(async move {
                    sleep(duration).await;
                    let _ = tx.send(RuntimeMessage::Resume(thread_id));
                });
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}
