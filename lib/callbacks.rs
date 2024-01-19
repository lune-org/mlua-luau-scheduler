use mlua::prelude::*;

type ValueCallback = Box<dyn for<'lua> Fn(&'lua Lua, LuaThread<'lua>, LuaValue<'lua>) + 'static>;
type ErrorCallback = Box<dyn for<'lua> Fn(&'lua Lua, LuaThread<'lua>, LuaError) + 'static>;

const FORWARD_VALUE_KEY: &str = "__runtime__forwardValue";
const FORWARD_ERROR_KEY: &str = "__runtime__forwardError";

pub struct Callbacks {
    on_value: Option<ValueCallback>,
    on_error: Option<ErrorCallback>,
}

impl Callbacks {
    pub fn new() -> Callbacks {
        Default::default()
    }

    pub fn on_value<F>(mut self, f: F) -> Self
    where
        F: Fn(&Lua, LuaThread, LuaValue) + 'static,
    {
        self.on_value.replace(Box::new(f));
        self
    }

    pub fn on_error<F>(mut self, f: F) -> Self
    where
        F: Fn(&Lua, LuaThread, LuaError) + 'static,
    {
        self.on_error.replace(Box::new(f));
        self
    }

    pub fn without_value_callback(mut self) -> Self {
        self.on_value.take();
        self
    }

    pub fn without_error_callback(mut self) -> Self {
        self.on_error.take();
        self
    }

    pub fn inject(self, lua: &Lua) {
        // Create functions to forward values & errors
        if let Some(f) = self.on_value {
            lua.set_named_registry_value(
                FORWARD_VALUE_KEY,
                lua.create_function(move |lua, (thread, val): (LuaThread, LuaValue)| {
                    f(lua, thread, val);
                    Ok(())
                })
                .expect("failed to create value callback function"),
            )
            .expect("failed to store value callback function");
        }

        if let Some(f) = self.on_error {
            lua.set_named_registry_value(
                FORWARD_ERROR_KEY,
                lua.create_function(move |lua, (thread, err): (LuaThread, LuaError)| {
                    f(lua, thread, err);
                    Ok(())
                })
                .expect("failed to create error callback function"),
            )
            .expect("failed to store error callback function");
        }
    }

    pub(crate) fn forward_value(lua: &Lua, thread: LuaThread, value: LuaValue) {
        if let Ok(f) = lua.named_registry_value::<LuaFunction>(FORWARD_VALUE_KEY) {
            f.call::<_, ()>((thread, value)).unwrap();
        }
    }

    pub(crate) fn forward_error(lua: &Lua, thread: LuaThread, error: LuaError) {
        if let Ok(f) = lua.named_registry_value::<LuaFunction>(FORWARD_ERROR_KEY) {
            f.call::<_, ()>((thread, error)).unwrap();
        }
    }
}

impl Default for Callbacks {
    fn default() -> Self {
        Callbacks {
            on_value: Some(Box::new(default_value_callback)),
            on_error: Some(Box::new(default_error_callback)),
        }
    }
}

fn default_value_callback(_: &Lua, _: LuaThread, _: LuaValue) {}
fn default_error_callback(_: &Lua, _: LuaThread, e: LuaError) {
    eprintln!("{e}");
}
