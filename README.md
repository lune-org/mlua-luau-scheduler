<!-- markdownlint-disable MD033 -->
<!-- markdownlint-disable MD041 -->

<h1 align="center">mlua-luau-runtime</h1>

<div align="center">
	<div>
		<a href="https://github.com/lune-org/mlua-luau-runtime/actions">
			<img src="https://shields.io/endpoint?url=https://badges.readysetplay.io/workflow/lune-org/mlua-luau-runtime/ci.yaml" alt="CI status" />
		</a>
		<a href="https://github.com/lune-org/mlua-luau-runtime/blob/main/LICENSE.txt">
			<img src="https://img.shields.io/github/license/lune-org/mlua-luau-runtime.svg?label=License&color=informational" alt="Crate license" />
		</a>
	</div>
</div>

<br/>

Luau-based async runtime, using [`mlua`](https://crates.io/crates/mlua) and [`async-executor`](https://crates.io/crates/async-executor).

## Example Usage

### 1. Import dependencies

```rs
use std::time::{Duration, Instant};
use std::io::ErrorKind;

use async_io::{block_on, Timer};
use async_fs::read_to_string;

use mlua::prelude::*;
use mlua_luau_runtime::*;
```

### 2. Set up Lua environment

```rs
let lua = Lua::new();

lua.globals().set(
    "sleep",
    lua.create_async_function(|_, duration: f64| async move {
        let before = Instant::now();
        let after = Timer::after(Duration::from_secs_f64(duration)).await;
        Ok((after - before).as_secs_f64())
    })?,
)?;

lua.globals().set(
    "readFile",
    lua.create_async_function(|lua, path: String| async move {
        // Spawn background task that does not take up resources on the lua thread
        // Normally, futures in mlua can not be shared across threads, but this can
        let task = lua.spawn(async move {
            match read_to_string(path).await {
                Ok(s) => Ok(Some(s)),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        });
        task.await.into_lua_err()
    })?,
)?;
```

### 3. Set up runtime, run threads

```rs
let rt = Runtime::new(&lua)?;

// We can create multiple lua threads ...
let sleepThread = lua.load("sleep(0.1)");
let fileThread = lua.load("readFile(\"Cargo.toml\")");

// ... spawn them both onto the runtime ...
rt.spawn_thread(sleepThread, ());
rt.spawn_thread(fileThread, ());

// ... and run until they finish
block_on(rt.run());
```
