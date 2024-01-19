use mlua::prelude::*;

type ErrorCallback = Box<dyn for<'lua> Fn(&'lua Lua, LuaThread<'lua>, LuaError) + 'static>;
type ValueCallback = Box<dyn for<'lua> Fn(&'lua Lua, LuaThread<'lua>, LuaValue<'lua>) + 'static>;

#[derive(Default)]
pub struct Callbacks {
    on_error: Option<ErrorCallback>,
    on_value: Option<ValueCallback>,
}

impl Callbacks {
    pub fn new() -> Callbacks {
        Default::default()
    }

    pub fn on_error<F>(mut self, f: F) -> Self
    where
        F: Fn(&Lua, LuaThread, LuaError) + 'static,
    {
        self.on_error.replace(Box::new(f));
        self
    }

    pub fn on_value<F>(mut self, f: F) -> Self
    where
        F: Fn(&Lua, LuaThread, LuaValue) + 'static,
    {
        self.on_value.replace(Box::new(f));
        self
    }

    pub fn inject(self, lua: &Lua) {
        // Create functions to forward errors & values
        if let Some(f) = self.on_error {
            lua.set_named_registry_value(
                "__forward__error",
                lua.create_function(move |lua, (thread, err): (LuaThread, LuaError)| {
                    f(lua, thread, err);
                    Ok(())
                })
                .expect("failed to create error callback function"),
            )
            .expect("failed to store error callback function");
        }

        if let Some(f) = self.on_value {
            lua.set_named_registry_value(
                "__forward__value",
                lua.create_function(move |lua, (thread, val): (LuaThread, LuaValue)| {
                    f(lua, thread, val);
                    Ok(())
                })
                .expect("failed to create value callback function"),
            )
            .expect("failed to store value callback function");
        }
    }

    pub(crate) fn forward_error(lua: &Lua, thread: LuaThread, error: LuaError) {
        if let Ok(f) = lua.named_registry_value::<LuaFunction>("__forward__error") {
            f.call::<_, ()>((thread, error)).unwrap();
        }
    }

    pub(crate) fn forward_value(lua: &Lua, thread: LuaThread, value: LuaValue) {
        if let Ok(f) = lua.named_registry_value::<LuaFunction>("__forward__value") {
            f.call::<_, ()>((thread, value)).unwrap();
        }
    }
}
