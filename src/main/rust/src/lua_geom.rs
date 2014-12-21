#![allow(non_snake_case)]
use core::prelude::*;
use core::{mem, ptr, raw};
use core::str;
use core::borrow::IntoCow;
use collections::string::String;
use collections::str::StrAllocating;
use libc::{c_char, c_void, size_t};

use lua::lib::raw::*;
use lua::aux::raw::*;
use lua::raw::*;
use luajit::*;
use luajit_constants::*;

use glcommon::{GLResult, MString};

use lua_callbacks::LuaCallback;

use lua_geom::SandboxMode::{Sandboxed, Unsandboxed};
use lua_geom::LuaValue::{RegistryValue, IndexValue};

//static mut GLDRAW_LUA_CREATE_SANDBOX: i32 = 0;
//static mut GLDRAW_LUA_STOPFNS: i32 = 0;
static LUA_FFI_SCRIPT: &'static str = include_str!("../includes/lua/ffi_loader.lua");
static LUA_RUNNER: &'static str = include_str!("../includes/lua/lua_runner.lua");
static LUA_INTERPOLATOR_DEFAULTS: &'static str = include_str!("../includes/lua/init_defaults.lua");

static mut STATIC_LUA: Option<*mut LuaInterpolatorState> = None;

struct LuaInterpolatorState {
    L: *mut lua_State,
    original_panicfn: lua_CFunction,
    create_sandbox_ref: RegistryRef,
    stopfns: RegistryRef,
}

macro_rules! assert_stacksize {
    ($L:expr, $body:expr) => (
        {
            let stacksize = lua_gettop($L);
            let result = $body;
            assert_eq!(stacksize, lua_gettop($L));
            result
        }
    )
}

macro_rules! safe_pop {
    ($L:expr, $body:expr) => (
        {
            assert!(lua_gettop($L) as u32 >= $body as u32);
            lua_pop($L, $body);
        }
    )
}

type ReaderState<'a> = (&'a str, bool);

#[deriving(Copy)]
enum SandboxMode {
    Sandboxed(LuaValue),
    Unsandboxed,
}

#[allow(raw_pointer_deriving)]
#[deriving(Copy)]
enum LuaValue {
    #[allow(dead_code)]
    RegistryValue(*mut c_void),
    IndexValue(i32),
}

struct RegistryRef {
    idx: i32,
}

impl RegistryRef {
    #[inline(always)]
    pub fn new(L: *mut lua_State) {
        let idx = luaL_ref(L, LUA_REGISTRYINDEX);
        RegistryRef { idx: idx }
    }
    #[inline(always)]
    pub fn push(&self, L: *mut lua_State) {
        lua_rawgeti(L, LUA_REGISTRYINDEX, self.idx);
    }
    #[inline(always)]
    pub unsafe fn destroy(&mut self, L: *mut lua_State) {
        luaL_unref(L, LUA_REGISTRYINDEX, self.idx);
    }
}

impl LuaValue {
    unsafe fn push_self(&self, L: *mut lua_State) {
        match self {
            &RegistryValue(key) => {
                lua_pushlightuserdata(L, key);
                lua_gettable(L, LUA_REGISTRYINDEX);
            },
            &IndexValue(idx) => {
                lua_pushvalue(L, idx);
            },
        }
    }
}

#[allow(unused)]
extern "C" fn stringreader(L: *mut lua_State, data: *mut c_void, size: *mut size_t) -> *const c_char {
    unsafe {
        let state: &mut ReaderState = mem::transmute(data);
        let (ref chars, ref mut done) = *state;
        if *done {
            ptr::null()
        } else {
            *done = true;
            *size = chars.len() as size_t;
            chars.as_ptr() as *const i8
        }
    }
}

unsafe fn get_lua<T: LuaCallback>(callback: T) -> GLResult<*mut lua_State> {
    match STATIC_LUA {
        Some(x) => Ok(x),
        None => {
            let lua = try!(LuaInterpolatorState::init_lua());
            STATIC_LUA = Some(lua);
            lua
        }
    }
}

#[no_mangle]
unsafe extern "C" fn panic_wrapper(L: *mut lua_State) -> i32 {
    loge!("inside lua panic handler!");
    let errorcstr = lua_tostring(L, -1);
    let errorstr = if errorcstr.is_null() { "" } else { str::from_c_str(errorcstr) };
    loge!("error is {}", errorstr);
    let panicfn = get_existing_lua().unwrap().original_panicfn;
    panicfn(L); // should never return
    -1
}

#[no_mangle]
pub unsafe fn rust_raise_lua_err(L: Option<*mut lua_State>, msg: &str) -> ! {
    let L = L.unwrap_or(get_existing_lua().unwrap());
    ::lua::raw::lua_pushlstring(L, msg.as_ptr() as *const i8, msg.len() as size_t);
    ::lua::raw::lua_error(L);
    panic!("luaL_error() returned, this should never happen!");
}

#[inline(always)]
pub unsafe fn get_existing_lua() -> Option<*mut lua_State> {
    STATIC_LUA
}

pub unsafe fn get_existing_lua_or_err() -> GLResult<*mut lua_State> {
    match get_existing_lua() {
        Some(lua) => Ok(lua),
        None => Err("couldn't get lua state!".into_cow()),
    }
}

pub unsafe fn load_lua_script(script: &str) -> GLResult<i32> {
    let state = try!(get_lua());
    state.load_lua_script(script);
}

fn log_err<T>(message: MString) -> GLResult<T> {
    loge!("{}", message.as_slice());
    Err(message)
}

pub unsafe fn do_interpolate_lua<T: LuaCallback>(script: &::luascript::LuaScript, callback: &mut T) -> GLResult<()> {
    let state = try!(get_existing_lua_or_err());
    state.do_interpolate_lua(script, callback)
}

pub unsafe fn destroy_lua_script(script: i32) {
    let state = get_existing_lua().unwrap();
    state.destroy_lua_script(script);
}

pub unsafe fn finish_lua_script<T: LuaCallback>(output: &mut T, script: &::luascript::LuaScript) -> GLResult<()> {
    let state = try!(get_existing_lua_or_err());
    state.finish_lua_script(output, script)
}

unsafe fn runstring(L: *mut lua_State, s: &str, filename: *const i8, env: SandboxMode) -> bool {
    let mut state: ReaderState = (s, false);
    let stateref: *mut c_void = mem::transmute(&mut state);
    if 0 != lua_load(L, stringreader, stateref, filename) {
        false
        //Err(format!("script failed to load: {}", err_to_str(L)))
    } else {
        if let Sandboxed(key) = env {
            key.push_self(L);
            lua_setfenv(L, -2);
        }
        let result = lua_pcall(L, 0, 0, 0);
        if 0 != result {
            false
            //Err(format!("script failed to run: {}", err_to_str(L)));
        } else {
            true
            //Ok(())
        }
    }
}

unsafe fn err_to_str(L: *mut lua_State) -> String {
    let mut size: size_t = 0;
    let strptr = lua_tolstring(L, -1, &mut size);
    let luastr: &str = mem::transmute(raw::Slice { data: strptr, len: size as uint });
    let result = luastr.into_string();
    safe_pop!(L, 1);
    result
}

impl LuaInterpolatorState {

    unsafe fn init_lua<T: LuaCallback>(callback: T) -> GLResult<LuaInterpolatorState<T>> {
        let L = luaL_newstate();
        let stacksize = lua_gettop(L);
        luaL_openlibs(L);

        luaJIT_setmode(L, 0, LUAJIT_MODE_ENGINE as i32|LUAJIT_MODE_ON as i32);

        if runstring(LUA_FFI_SCRIPT, cstr!("built-in ffi init script"), Unsandboxed) {
            logi!("ffi init script loaded");
            lua_getglobal(L, cstr!("create_sandbox"));
            let create_sandbox = RegistryRef::new(L);

            lua_newtable(L);
            let stopfns = RegistryRef::new(L);

            let original_panicfn = lua_atpanic(L, panic_wrapper);
            let state = LuaInterpolatorState {
                L: L,
                original_panicfn: original_panicfn,
                create_sandbox_ref: create_sandbox,
                stopfns: stopfns,
            };
            
            assert_eq!(stacksize, lua_gettop(L));
            Ok(state)
        } else {
            let err = format!("ffi init script failed to load: {}\nThis should never happen!", err_to_str(L)).into_cow();
            lua_close(L);
            log_err(err)
        }
    }

    unsafe fn create_sandbox(&mut self) {
        let L = self.L;
        self.create_sandbox_ref.push(L);
        lua_pcall(L, 0, 1, 0);
    }

    unsafe fn save_ondone(&mut self, key: i32, sandbox: LuaValue) -> GLResult<()> {
        let L = self.L;
        logi!("saving ondone() method");
        let stacksize = lua_gettop(L);
        sandbox.push_self(L); {
            self.stopfns.push(L); {
                // stack holds sandbox -- stopfns
                lua_getfield(L, -2, cstr!("ondone")); {
                    // stack holds sandbox -- stopfns -- ondone
                    if !lua_isfunction(L, -1) {
                        safe_pop!(L, 3);
                        assert_eq!(stacksize, lua_gettop(L));
                        return log_err("ondone not defined.\nThis should never happen!".into_cow());
                    }
                    logi!("saving ondone method to 0x{:x} in gldraw_lua_stopfns", key);
                    lua_rawseti(L, -2, key);
                }
                // stack holds sandbox -- stopfns
                safe_pop!(L, 2);
                assert_eq!(stacksize, lua_gettop(L));
                Ok(())
            }
        }
    }

    pub unsafe fn load_lua_script(&mut self, script: &str) -> GLResult<i32> {
        let L = self.L;
        logi!("got lua");
        let stacksize = lua_gettop(L);
        let result = self.load_lua_script_internal(script);
        assert_eq!(stacksize, lua_gettop(L));
        result
    }

    unsafe fn load_lua_script_internal(&mut self, script: &str) -> GLResult<i32> {
        let L = self.L;
        let stacksize = lua_gettop(L);

        self.create_sandbox();
        let key = {
            let sandbox_stackpos = lua_gettop(L);
            let sandbox_idx = IndexValue(sandbox_stackpos);

            let (width, height) = self.dimensions;
            lua_pushinteger(L, width);
            lua_setfield(L, sandbox_stackpos, cstr!("width"));
            lua_pushinteger(L, height);
            lua_setfield(L, sandbox_stackpos, cstr!("height"));

            if !runstring(L, LUA_INTERPOLATOR_DEFAULTS, cstr!("interpolator defaults"), Sandboxed(sandbox_idx)) {
                let err = format!("default loader failed to load: {}\nThis should never happen!", err_to_str(L)).into_cow();
                safe_pop!(L, 1);
                assert_eq!(stacksize, lua_gettop(L));
                return log_err(err);
            }

            if !runstring(L, script, cstr!("interpolator script"), Sandboxed(sandbox_idx)) {
                let err = format!("script failed to load: {}", err_to_str(L)).into_cow();
                safe_pop!(L, 1);
                assert_eq!(stacksize, lua_gettop(L));
                return log_err(err);
            }

            sandbox_idx.push_self(L); {
                lua_getfield(L, -1, cstr!("main")); {
                    if !lua_isfunction(L, -1) {
                        safe_pop!(L, 3);
                        assert_eq!(stacksize, lua_gettop(L));
                        return log_err("no main function defined :(".into_cow());
                    }
                    luaJIT_setmode(L, 0, LUAJIT_MODE_ENGINE as i32|LUAJIT_MODE_ON as i32);
                    safe_pop!(L, 1);
                }

                // make values defined in script available to lua_runner
                lua_setglobal(L, cstr!("callbacks"));
            }

            // TODO consider compiling runner once
            if !runstring(L, LUA_RUNNER, cstr!("built-in lua_runner script"), Unsandboxed) {
                let err = format!("lua runner failed to load: {}\n This should never happen!", err_to_str(L)).into_cow();
                safe_pop!(L, 1);
                assert_eq!(stacksize, lua_gettop(L));
                return log_err(err);
            }

            lua_getglobal(L, cstr!("runmain"));
            let key = {
                if !lua_isfunction(L, -1) {
                    safe_pop!(L, 2);
                    assert_eq!(stacksize, lua_gettop(L));
                    return log_err("runmain not defined.\nThis should never happen!".into_cow());
                }
                luaJIT_setmode(L, 0, LUAJIT_MODE_ENGINE as i32|LUAJIT_MODE_ON as i32);

                let key = luaL_ref(L, LUA_REGISTRYINDEX);

                if let Err(msg) = self.save_ondone(key, sandbox_idx) {
                    safe_pop!(L, 2);
                    luaL_unref(L, LUA_REGISTRYINDEX, key);
                    assert_eq!(stacksize, lua_gettop(L));
                    return Err(msg);
                }

                key
            };

            safe_pop!(L, 1);
            key
        };

        assert_eq!(stacksize, lua_gettop(L));
        Ok(key)
    }

    pub unsafe fn finish_lua_script<T: LuaCallback>(&mut self, output: &mut T, script: &::luascript::LuaScript) -> GLResult<()> {
        let L = self.L;
        let stacksize = lua_gettop(L);
        self.stopfns.push(L);
        let result = {
            // stack is stopfns
            lua_rawgeti(L, -1, script.get_key());
            // stack is stopfns -- stopfn
            logi!("calling lua ondone()");
            let result = match lua_pcall(L, 0, 0, 0) {
                0 => Ok(()),
                _ => {
                    log_err(format!("ondone() script failed to run: {}", err_to_str(L)).into_cow())
                }
            };
            safe_pop!(L, 1);
            result
        };
        assert_eq!(stacksize, lua_gettop(L));
        result
    }

    pub unsafe fn destroy_lua_script(&mut self, key: i32) {
        let L = self.L;
        let stacksize = lua_gettop(L);
        luaL_unref(L, LUA_REGISTRYINDEX, key);
        self.stopfns.push(L); {
            lua_pushnil(L);
            lua_rawseti(L, -2, key);
            safe_pop!(L, 1);
        }
        assert_eq!(stacksize, lua_gettop(L));
    }

    #[inline]
    pub unsafe fn push_lua_script(&mut self, key: i32) {
        let L = self.L;
        lua_pushlightuserdata(L, key as *mut c_void);
    }

    pub unsafe fn do_interpolate_lua<T: LuaCallback>(&mut self, script: &::luascript::LuaScript, callback: &mut T) -> GLResult<()> {
        let L = self.L;
        let stacksize = lua_gettop(L);
        lua_rawgeti(L, LUA_REGISTRYINDEX, script.get_key());

        lua_pushlightuserdata(L, callback as *mut T as *mut c_void);
        lua_setglobal(L, cstr!("output"));

        let result = match lua_pcall(L, 0, 0, 0) {
            0 => Ok(()),
            _ => log_err(format!("script failed to run: {}", err_to_str(L)).into_cow()),
        };
        assert_eq!(stacksize, lua_gettop(L));
        result
    }
}
