use std::{rc::Rc, sync::Arc};

use mlua::prelude::*;
use smol::*;

struct LuaSmol<'ex> {
    lua_exec: Rc<LocalExecutor<'ex>>,
    main_exec: Arc<Executor<'ex>>,
}

// HACK: self_cell is not actually used to make a self-referential struct here,
// it is instead used to guarantee the lifetime of the executors. It does not
// need to refer to Lua during construction at all but the end result is the
// same and we let the self_cell crate handle all the unsafe code for us.
self_cell::self_cell!(
    struct LuaExecutorInner {
        owner: Rc<Lua>,

        #[not_covariant]
        dependent: LuaSmol,
    }
);

impl LuaExecutorInner {
    fn create(lua: Rc<Lua>) -> Self {
        LuaExecutorInner::new(lua, |_| {
            let lua_exec = Rc::new(LocalExecutor::new());
            let main_exec = Arc::new(Executor::new());
            LuaSmol {
                lua_exec,
                main_exec,
            }
        })
    }
}

pub struct LuaExecutor {
    _lua: Rc<Lua>,
    inner: LuaExecutorInner,
}

impl LuaExecutor {
    pub fn new(lua: Rc<Lua>) -> Self {
        Self {
            _lua: Rc::clone(&lua),
            inner: LuaExecutorInner::create(lua),
        }
    }

    pub fn run<'outer_fn, F>(&'outer_fn self, futures_spawner: F) -> LuaResult<()>
    where
        F: for<'lua> FnOnce(
            &'lua Lua,
            &'outer_fn LocalExecutor<'lua>,
            &'outer_fn Executor<'lua>,
        ) -> LuaResult<()>,
    {
        self.inner.with_dependent(|lua, rt_executors| {
            // 1. Spawn futures using the provided function
            let lua_exec = &rt_executors.lua_exec;
            let main_exec = &rt_executors.main_exec;
            futures_spawner(lua, lua_exec, main_exec)?;
            // 2. Run them all until lua executor completes
            block_on(main_exec.run(async {
                while !lua_exec.is_empty() {
                    lua_exec.tick().await;
                }
            }));
            // 3. Yay!
            Ok(())
        })
    }
}
