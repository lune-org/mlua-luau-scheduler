use mlua::prelude::*;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ThreadId(usize);

impl ThreadId {
    fn new(value: &LuaThread) -> Self {
        // HACK: We rely on the debug format of mlua
        // thread refs here, but currently this is the
        // only way to get a proper unique id using mlua
        let addr_string = format!("{value:?}");
        let addr = addr_string
            .strip_prefix("Thread(Ref(0x")
            .expect("Invalid thread address format - unknown prefix")
            .split_once(')')
            .map(|(s, _)| s)
            .expect("Invalid thread address format - missing ')'");
        let id = usize::from_str_radix(addr, 16)
            .expect("Failed to parse thread address as hexadecimal into usize");
        Self(id)
    }
}

impl From<LuaThread<'_>> for ThreadId {
    fn from(value: LuaThread) -> Self {
        Self::new(&value)
    }
}

impl From<&LuaThread<'_>> for ThreadId {
    fn from(value: &LuaThread) -> Self {
        Self::new(value)
    }
}
