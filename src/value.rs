#![allow(dead_code)]

use std::time::Duration;

use mlua::prelude::*;

#[derive(Debug, Default)]
pub enum AsyncValue {
    #[default]
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl IntoLua<'_> for AsyncValue {
    #[inline]
    fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
        match self {
            AsyncValue::Nil => Ok(LuaValue::Nil),
            AsyncValue::Bool(b) => Ok(LuaValue::Boolean(b)),
            AsyncValue::Number(n) => Ok(LuaValue::Number(n)),
            AsyncValue::String(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
            AsyncValue::Bytes(b) => Ok(LuaValue::String(lua.create_string(&b)?)),
        }
    }
}

// Primitives

impl From<()> for AsyncValue {
    #[inline]
    fn from(_: ()) -> Self {
        AsyncValue::Nil
    }
}

impl From<bool> for AsyncValue {
    #[inline]
    fn from(b: bool) -> Self {
        AsyncValue::Bool(b)
    }
}

impl From<u8> for AsyncValue {
    #[inline]
    fn from(u: u8) -> Self {
        AsyncValue::Number(u as f64)
    }
}

impl From<u16> for AsyncValue {
    #[inline]
    fn from(u: u16) -> Self {
        AsyncValue::Number(u as f64)
    }
}

impl From<u32> for AsyncValue {
    #[inline]
    fn from(u: u32) -> Self {
        AsyncValue::Number(u as f64)
    }
}

impl From<u64> for AsyncValue {
    #[inline]
    fn from(u: u64) -> Self {
        AsyncValue::Number(u as f64)
    }
}

impl From<i8> for AsyncValue {
    #[inline]
    fn from(i: i8) -> Self {
        AsyncValue::Number(i as f64)
    }
}

impl From<i16> for AsyncValue {
    #[inline]
    fn from(i: i16) -> Self {
        AsyncValue::Number(i as f64)
    }
}

impl From<i32> for AsyncValue {
    #[inline]
    fn from(i: i32) -> Self {
        AsyncValue::Number(i as f64)
    }
}

impl From<i64> for AsyncValue {
    #[inline]
    fn from(i: i64) -> Self {
        AsyncValue::Number(i as f64)
    }
}

impl From<f32> for AsyncValue {
    #[inline]
    fn from(n: f32) -> Self {
        AsyncValue::Number(n as f64)
    }
}

impl From<f64> for AsyncValue {
    #[inline]
    fn from(n: f64) -> Self {
        AsyncValue::Number(n)
    }
}

impl From<String> for AsyncValue {
    #[inline]
    fn from(s: String) -> Self {
        AsyncValue::String(s)
    }
}

impl From<&String> for AsyncValue {
    #[inline]
    fn from(s: &String) -> Self {
        AsyncValue::String(s.to_owned())
    }
}

impl From<&str> for AsyncValue {
    #[inline]
    fn from(s: &str) -> Self {
        AsyncValue::String(s.to_owned())
    }
}

impl From<Vec<u8>> for AsyncValue {
    #[inline]
    fn from(b: Vec<u8>) -> Self {
        AsyncValue::Bytes(b)
    }
}

impl From<&Vec<u8>> for AsyncValue {
    #[inline]
    fn from(b: &Vec<u8>) -> Self {
        AsyncValue::Bytes(b.to_owned())
    }
}

impl From<&[u8]> for AsyncValue {
    #[inline]
    fn from(b: &[u8]) -> Self {
        AsyncValue::Bytes(b.to_owned())
    }
}

// Other types

impl From<Duration> for AsyncValue {
    #[inline]
    fn from(d: Duration) -> Self {
        AsyncValue::Number(d.as_secs_f64())
    }
}

// Multi args

#[derive(Debug, Default)]
pub struct AsyncValues {
    inner: Vec<AsyncValue>,
}

impl AsyncValues {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl IntoLuaMulti<'_> for AsyncValues {
    #[inline]
    fn into_lua_multi(self, lua: &Lua) -> LuaResult<LuaMultiValue> {
        Ok(LuaMultiValue::from_vec(
            self.inner
                .into_iter()
                .map(|arg| arg.into_lua(lua))
                .collect::<LuaResult<Vec<_>>>()?,
        ))
    }
}

// Boilerplate

impl<T> From<T> for AsyncValues
where
    T: Into<AsyncValue>,
{
    #[inline]
    fn from(t: T) -> Self {
        AsyncValues {
            inner: vec![t.into()],
        }
    }
}

impl<T0, T1> From<(T0, T1)> for AsyncValues
where
    T0: Into<AsyncValue>,
    T1: Into<AsyncValue>,
{
    #[inline]
    fn from((t0, t1): (T0, T1)) -> Self {
        AsyncValues {
            inner: vec![t0.into(), t1.into()],
        }
    }
}

impl<T0, T1, T2> From<(T0, T1, T2)> for AsyncValues
where
    T0: Into<AsyncValue>,
    T1: Into<AsyncValue>,
    T2: Into<AsyncValue>,
{
    #[inline]
    fn from((t0, t1, t2): (T0, T1, T2)) -> Self {
        AsyncValues {
            inner: vec![t0.into(), t1.into(), t2.into()],
        }
    }
}

impl<T0, T1, T2, T3> From<(T0, T1, T2, T3)> for AsyncValues
where
    T0: Into<AsyncValue>,
    T1: Into<AsyncValue>,
    T2: Into<AsyncValue>,
    T3: Into<AsyncValue>,
{
    #[inline]
    fn from((t0, t1, t2, t3): (T0, T1, T2, T3)) -> Self {
        AsyncValues {
            inner: vec![t0.into(), t1.into(), t2.into(), t3.into()],
        }
    }
}

impl<T0, T1, T2, T3, T4> From<(T0, T1, T2, T3, T4)> for AsyncValues
where
    T0: Into<AsyncValue>,
    T1: Into<AsyncValue>,
    T2: Into<AsyncValue>,
    T3: Into<AsyncValue>,
    T4: Into<AsyncValue>,
{
    #[inline]
    fn from((t0, t1, t2, t3, t4): (T0, T1, T2, T3, T4)) -> Self {
        AsyncValues {
            inner: vec![t0.into(), t1.into(), t2.into(), t3.into(), t4.into()],
        }
    }
}

impl<T0, T1, T2, T3, T4, T5> From<(T0, T1, T2, T3, T4, T5)> for AsyncValues
where
    T0: Into<AsyncValue>,
    T1: Into<AsyncValue>,
    T2: Into<AsyncValue>,
    T3: Into<AsyncValue>,
    T4: Into<AsyncValue>,
    T5: Into<AsyncValue>,
{
    #[inline]
    fn from((t0, t1, t2, t3, t4, t5): (T0, T1, T2, T3, T4, T5)) -> Self {
        AsyncValues {
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
