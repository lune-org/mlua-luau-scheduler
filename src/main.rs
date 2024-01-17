use gxhash::GxHashMap;
use mlua::prelude::*;
use tokio::{
    io::{self, AsyncWriteExt},
    runtime::Runtime as TokioRuntime,
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver},
    task::{spawn_blocking, LocalSet},
    time::{sleep, Instant},
};

mod error_storage;
mod lua;
mod lua_ext;
mod message;
mod stats;
mod thread_id;
mod value;

use error_storage::*;
use lua::*;
use message::*;
use stats::*;
use thread_id::*;
use value::*;

const NUM_TEST_BATCHES: usize = 20;
const NUM_TEST_THREADS: usize = 50_000;

const MAIN_CHUNK: &str = r#"
wait(0.01 * math.random())
"#;

const WAIT_IMPL: &str = r#"
__scheduler__resumeAfter(...)
return coroutine.yield()
"#;

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
    let lua = create_lua(tx.clone())?;

    let error_storage = ErrorStorage::new();
    let error_storage_interrupt = error_storage.clone();
    lua.set_interrupt(move |_| match error_storage_interrupt.take() {
        Some(e) => Err(e),
        None => Ok(LuaVmState::Continue),
    });

    lua.globals()
        .set("wait", lua.load(WAIT_IMPL).into_function()?)?;

    let mut yielded_threads = GxHashMap::default();
    let mut runnable_threads = GxHashMap::default();

    println!("Running {NUM_TEST_BATCHES} batches");
    for _ in 0..NUM_TEST_BATCHES {
        let main_fn = lua.load(MAIN_CHUNK).into_function()?;
        for _ in 0..NUM_TEST_THREADS {
            let thread = lua.create_thread(main_fn.clone())?;
            runnable_threads.insert(ThreadId::from(&thread), (thread, Ok(AsyncValues::new())));
        }

        loop {
            // Runnable / yielded threads may be empty because of cancellation
            if runnable_threads.is_empty() && yielded_threads.is_empty() {
                break;
            }

            // Resume as many threads as possible
            for (thread_id, (thread, res)) in runnable_threads.drain() {
                stats.incr(StatsCounter::ThreadResumed);
                // NOTE: If we got an error we don't need to resume with any args
                let args = match res {
                    Ok(a) => a,
                    Err(e) => {
                        error_storage.replace(e);
                        AsyncValues::from(())
                    }
                };
                if let Err(e) = thread.resume::<_, ()>(args) {
                    tx.send(Message::WriteError(e)).unwrap();
                } else if thread.status() == LuaThreadStatus::Resumable {
                    yielded_threads.insert(thread_id, thread);
                }
            }

            if yielded_threads.is_empty() {
                break; // All threads ran, and we don't have any async task that can spawn more
            }

            // Set up message processor - we mutably borrow both yielded_threads and runnable_threads
            // so we can't really do this outside of the loop, but it compiles down to the same thing
            let mut process_message = |message| match message {
                Message::Resume(thread_id, res) => {
                    if let Some(thread) = yielded_threads.remove(&thread_id) {
                        runnable_threads.insert(thread_id, (thread, res));
                    }
                }
                Message::Cancel(thread_id) => {
                    yielded_threads.remove(&thread_id);
                    runnable_threads.remove(&thread_id);
                    stats.incr(StatsCounter::ThreadCancelled);
                }
                _ => unreachable!(),
            };

            // Wait for at least one message, but try to receive as many as possible
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
    // Give stdio its own task, we don't need it to block the scheduler
    let (tx_stdout, rx_stdout) = unbounded_channel();
    let (tx_stderr, rx_stderr) = unbounded_channel();
    let forward_stdout = |data| tx_stdout.send(data).ok();
    let forward_stderr = |data| tx_stderr.send(data).ok();
    spawn(async move {
        if let Err(e) = async_stdio_task(rx_stdout, rx_stderr).await {
            eprintln!("Stdio fatal error: {e}");
        }
    });

    // Set up message processor
    let process_message = |message| {
        match message {
            Message::Sleep(_, _, _) => stats.incr(StatsCounter::ThreadSlept),
            Message::WriteError(_) => stats.incr(StatsCounter::ThreadErrored),
            Message::WriteStdout(_) => stats.incr(StatsCounter::WriteStdout),
            Message::WriteStderr(_) => stats.incr(StatsCounter::WriteStderr),
            _ => unreachable!(),
        }

        match message {
            Message::Sleep(thread_id, yielded_at, duration) => {
                let tx = tx.clone();
                spawn(async move {
                    sleep(duration).await;
                    let elapsed = Instant::now() - yielded_at;
                    tx.send(Message::Resume(thread_id, Ok(AsyncValues::from(elapsed))))
                });
            }
            Message::WriteError(e) => {
                forward_stderr(b"Lua error: ".to_vec());
                forward_stderr(e.to_string().as_bytes().to_vec());
            }
            Message::WriteStdout(data) => {
                forward_stdout(data);
            }
            Message::WriteStderr(data) => {
                forward_stderr(data);
            }
            _ => unreachable!(),
        }
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

async fn async_stdio_task(
    mut rx_stdout: UnboundedReceiver<Vec<u8>>,
    mut rx_stderr: UnboundedReceiver<Vec<u8>>,
) -> LuaResult<()> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    loop {
        select! {
            data = rx_stdout.recv() => match data {
                None => break, // Main task exited
                Some(data) => {
                    stdout.write_all(&data).await?;
                    stdout.flush().await?;
                }
            },
            data = rx_stderr.recv() => match data {
                None => break, // Main task exited
                Some(data) => {
                    stderr.write_all(&data).await?;
                    stderr.flush().await?;
                }
            }
        }
    }

    Ok(())
}
