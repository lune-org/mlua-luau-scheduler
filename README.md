<!-- markdownlint-disable MD033 -->
<!-- markdownlint-disable MD041 -->

<h1 align="center">smol-mlua</h1>

<div align="center">
	<div>
		<a href="https://github.com/lune-org/smol-mlua/actions">
			<img src="https://shields.io/endpoint?url=https://badges.readysetplay.io/workflow/lune-org/smol-mlua/ci.yaml" alt="CI status" />
		</a>
		<a href="https://github.com/lune-org/smol-mlua/blob/main/LICENSE.txt">
			<img src="https://img.shields.io/github/license/lune-org/smol-mlua.svg?label=License&color=informational" alt="Crate license" />
		</a>
	</div>
</div>

<br/>

Integration between [smol] and [mlua] that provides a fully functional and asynchronous Luau runtime using smol executor(s).

[smol]: https://crates.io/crates/smol
[mlua]: https://crates.io/crates/mlua

## Example Usage

### 1. Import dependencies

```rs
use std::time::{Duration, Instant};

use mlua::prelude::*;
use smol::{Timer, io, fs::read_to_string}
use smol_mlua::Runtime;
```

### 2. Set up lua environment

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
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        });
        task.await.into_lua_err()
    })?,
)?;
```

### 3. Run

```rs
let rt = Runtime::new(&lua)?;

// We can create multiple lua threads ...
let sleepThread = lua.load("sleep(0.1)");
let fileThread = lua.load("readFile(\"Cargo.toml\")");

// ... spawn them both onto the runtime ...
rt.spawn_thread(sleepThread, ());
rt.spawn_thread(fileThread, ());

// ... and run either async or blocking, until they finish
rt.run_async().await;
rt.run_blocking();
```
