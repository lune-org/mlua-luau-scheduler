#![allow(dead_code)]

use std::time::Duration;

use mlua::prelude::*;

#[derive(Debug, Default)]
pub enum Arg {
    #[default]
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
}

impl IntoLua<'_> for Arg {
    #[inline]
    fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
        match self {
            Arg::Nil => Ok(LuaValue::Nil),
            Arg::Bool(b) => Ok(LuaValue::Boolean(b)),
            Arg::Number(n) => Ok(LuaValue::Number(n)),
            Arg::String(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
        }
    }
}

// Primitives

impl From<()> for Arg {
    #[inline]
    fn from(_: ()) -> Self {
        Arg::Nil
    }
}

impl From<bool> for Arg {
    #[inline]
    fn from(b: bool) -> Self {
        Arg::Bool(b)
    }
}

impl From<f64> for Arg {
    #[inline]
    fn from(n: f64) -> Self {
        Arg::Number(n)
    }
}

impl From<String> for Arg {
    #[inline]
    fn from(s: String) -> Self {
        Arg::String(s)
    }
}

// Other types

impl From<Duration> for Arg {
    #[inline]
    fn from(d: Duration) -> Self {
        Arg::Number(d.as_secs_f64())
    }
}

// Multi args

#[derive(Debug, Default)]
pub struct Args {
    inner: Vec<Arg>,
}

impl Args {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl IntoLuaMulti<'_> for Args {
    #[inline]
    fn into_lua_multi(self, lua: &Lua) -> LuaResult<LuaMultiValue> {
        let mut values = Vec::new();
        for arg in self.inner {
            values.push(arg.into_lua(lua)?);
        }
        Ok(LuaMultiValue::from_vec(values))
    }
}

// Boilerplate

impl<T> From<T> for Args
where
    T: Into<Arg>,
{
    #[inline]
    fn from(t: T) -> Self {
        Args {
            inner: vec![t.into()],
        }
    }
}

impl<T0, T1> From<(T0, T1)> for Args
where
    T0: Into<Arg>,
    T1: Into<Arg>,
{
    #[inline]
    fn from((t0, t1): (T0, T1)) -> Self {
        Args {
            inner: vec![t0.into(), t1.into()],
        }
    }
}

impl<T0, T1, T2> From<(T0, T1, T2)> for Args
where
    T0: Into<Arg>,
    T1: Into<Arg>,
    T2: Into<Arg>,
{
    #[inline]
    fn from((t0, t1, t2): (T0, T1, T2)) -> Self {
        Args {
            inner: vec![t0.into(), t1.into(), t2.into()],
        }
    }
}

impl<T0, T1, T2, T3> From<(T0, T1, T2, T3)> for Args
where
    T0: Into<Arg>,
    T1: Into<Arg>,
    T2: Into<Arg>,
    T3: Into<Arg>,
{
    #[inline]
    fn from((t0, t1, t2, t3): (T0, T1, T2, T3)) -> Self {
        Args {
            inner: vec![t0.into(), t1.into(), t2.into(), t3.into()],
        }
    }
}

impl<T0, T1, T2, T3, T4> From<(T0, T1, T2, T3, T4)> for Args
where
    T0: Into<Arg>,
    T1: Into<Arg>,
    T2: Into<Arg>,
    T3: Into<Arg>,
    T4: Into<Arg>,
{
    #[inline]
    fn from((t0, t1, t2, t3, t4): (T0, T1, T2, T3, T4)) -> Self {
        Args {
            inner: vec![t0.into(), t1.into(), t2.into(), t3.into(), t4.into()],
        }
    }
}

impl<T0, T1, T2, T3, T4, T5> From<(T0, T1, T2, T3, T4, T5)> for Args
where
    T0: Into<Arg>,
    T1: Into<Arg>,
    T2: Into<Arg>,
    T3: Into<Arg>,
    T4: Into<Arg>,
    T5: Into<Arg>,
{
    #[inline]
    fn from((t0, t1, t2, t3, t4, t5): (T0, T1, T2, T3, T4, T5)) -> Self {
        Args {
            inner: vec![
                t0.into(),
                t1.into(),
                t2.into(),
                t3.into(),
                t4.into(),
                t5.into(),
            ],
        }
    }
}
