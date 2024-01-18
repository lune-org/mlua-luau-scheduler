use mlua::prelude::*;

#[derive(Debug)]
pub struct ThreadWithArgs {
    key_thread: LuaRegistryKey,
    key_args: LuaRegistryKey,
}

impl ThreadWithArgs {
    pub fn new<'lua>(lua: &'lua Lua, thread: LuaThread<'lua>, args: LuaMultiValue<'lua>) -> Self {
        let args_vec = args.into_vec();

        let key_thread = lua
            .create_registry_value(thread)
            .expect("Failed to store thread in registry - out of memory");
        let key_args = lua
            .create_registry_value(args_vec)
            .expect("Failed to store thread args in registry - out of memory");

        Self {
            key_thread,
            key_args,
        }
    }

    pub fn into_inner(self, lua: &Lua) -> (LuaThread<'_>, LuaMultiValue<'_>) {
        let thread = lua
            .registry_value(&self.key_thread)
            .expect("Failed to get thread from registry");
        let args_vec = lua
            .registry_value(&self.key_args)
            .expect("Failed to get thread args from registry");

        let args = LuaMultiValue::from_vec(args_vec);

        lua.remove_registry_value(self.key_thread)
            .expect("Failed to remove thread from registry");
        lua.remove_registry_value(self.key_args)
            .expect("Failed to remove thread args from registry");

        (thread, args)
    }
}
