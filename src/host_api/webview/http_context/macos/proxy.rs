//! Runtime-bound Network proxy functions: no macOS 14 symbol is strongly linked.
use super::ContextError;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObjectProtocol},
    sel,
};
use objc2_foundation::NSArray;
use objc2_web_kit::WKWebsiteDataStore;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr::NonNull,
};

type CreateEndpoint = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut AnyObject;
type CreateProxy = unsafe extern "C" fn(*mut AnyObject) -> *mut AnyObject;
type Credentials = unsafe extern "C" fn(*mut AnyObject, *const c_char, *const c_char);
type Failover = unsafe extern "C" fn(*mut AnyObject, bool);
type Clear = unsafe extern "C" fn(*mut AnyObject);

pub(super) struct Network {
    endpoint: CreateEndpoint,
    proxy: CreateProxy,
    credentials: Credentials,
    failover: Failover,
    clear_match: Clear,
    clear_excluded: Clear,
}
impl Network {
    pub(super) fn load(store: &WKWebsiteDataStore) -> Result<&'static Self, ContextError> {
        if !store.respondsToSelector(sel!(setProxyConfigurations:)) {
            return Err(ContextError::Unsupported);
        }
        static NETWORK: std::sync::OnceLock<Result<Network, ContextError>> =
            std::sync::OnceLock::new();
        NETWORK
            .get_or_init(Self::open)
            .as_ref()
            .map_err(|error| *error)
    }
    fn open() -> Result<Self, ContextError> {
        // The framework exists on macOS 11. Keep its handle for process lifetime: callbacks and
        // retained native proxy objects may outlive this value. No mutable global state is used.
        // SAFETY: constant NUL-terminated path and supported POSIX flags.
        let handle = unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/Network.framework/Network".as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_LOCAL,
            )
        };
        if handle.is_null() {
            return Err(ContextError::Unsupported);
        }
        fn symbol(handle: *mut c_void, name: &CStr) -> Result<NonNull<c_void>, ContextError> {
            // SAFETY: handle from dlopen stays open and name is NUL-terminated.
            NonNull::new(unsafe { libc::dlsym(handle, name.as_ptr()) })
                .ok_or(ContextError::Unsupported)
        }
        macro_rules! bind {
            ($name:literal, $ty:ty) => {{
                let pointer = symbol(handle, $name)?;
                // SAFETY: checked symbol with the matching public SDK function signature.
                unsafe { std::mem::transmute::<NonNull<c_void>, $ty>(pointer) }
            }};
        }
        let network = Self {
            endpoint: bind!(c"nw_endpoint_create_host", CreateEndpoint),
            proxy: bind!(c"nw_proxy_config_create_socksv5", CreateProxy),
            credentials: bind!(c"nw_proxy_config_set_username_and_password", Credentials),
            failover: bind!(c"nw_proxy_config_set_failover_allowed", Failover),
            clear_match: bind!(c"nw_proxy_config_clear_match_domains", Clear),
            clear_excluded: bind!(c"nw_proxy_config_clear_excluded_domains", Clear),
        };
        Ok(network)
    }
    pub(super) fn configure(
        &self,
        store: &WKWebsiteDataStore,
        route: &ProxyRoute,
    ) -> Result<(), ContextError> {
        let port = CString::new(route.port.to_string()).map_err(|_| ContextError::InvalidProxy)?;
        // SAFETY: checked public Network signature with live C strings, returns +1.
        let endpoint_pointer = unsafe { (self.endpoint)(c"127.0.0.1".as_ptr(), port.as_ptr()) };
        // SAFETY: Network objects obey Objective-C ownership on Darwin; transfer the +1.
        let endpoint =
            unsafe { Retained::from_raw(endpoint_pointer) }.ok_or(ContextError::Native)?;
        // SAFETY: checked public Network signature and live retained host endpoint, returns +1.
        let proxy_pointer = unsafe { (self.proxy)(Retained::as_ptr(&endpoint).cast_mut()) };
        // SAFETY: transfer the returned proxy's +1 ownership exactly once.
        let proxy = unsafe { Retained::from_raw(proxy_pointer) }.ok_or(ContextError::Native)?;
        let pointer = Retained::as_ptr(&proxy).cast_mut();
        // SAFETY: checked SDK function types, owned proxy and live C strings.
        unsafe {
            (self.credentials)(pointer, route.username.as_ptr(), route.password.as_ptr());
        }
        // SAFETY: owned proxy. Disable advertised failover; implicit loopback bypass remains
        // an observed platform constraint, so this component must not activate App HTTP.
        unsafe {
            (self.failover)(pointer, false);
        }
        // SAFETY: empty match-domain and exclusion lists use the default all-hosts route.
        unsafe {
            (self.clear_match)(pointer);
        }
        // SAFETY: owned proxy.
        unsafe {
            (self.clear_excluded)(pointer);
        }
        let proxies = NSArray::from_slice(&[&*proxy]);
        // SAFETY: selector availability checked by load; store copies array of nw_proxy_config_t.
        unsafe {
            let _: () = msg_send![store, setProxyConfigurations: &*proxies];
        }
        Ok(())
    }
}
/// Credentials stay native-only. Deliberately no Debug/Serialize implementation.
pub struct ProxyRoute {
    port: u16,
    username: CString,
    password: CString,
}
impl ProxyRoute {
    /// Accept only the endpoint's literal loopback SOCKS URI, never PAC/remote/direct routes.
    pub fn from_endpoint(uri: &str) -> Result<Self, ContextError> {
        let value = uri
            .strip_prefix("socks5://")
            .ok_or(ContextError::InvalidProxy)?;
        let (credentials, address) = value.split_once('@').ok_or(ContextError::InvalidProxy)?;
        let (username, password) = credentials
            .split_once(':')
            .ok_or(ContextError::InvalidProxy)?;
        let port = address
            .strip_prefix("127.0.0.1:")
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|p| *p != 0)
            .ok_or(ContextError::InvalidProxy)?;
        // Endpoint credentials are ASCII hex. Refuse separators, escaping, NUL and overlong auth.
        let valid = |s: &str| {
            !s.is_empty()
                && s.len() <= 255
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        if !valid(username) || !valid(password) {
            return Err(ContextError::InvalidProxy);
        }
        Ok(Self {
            port,
            username: CString::new(username).map_err(|_| ContextError::InvalidProxy)?,
            password: CString::new(password).map_err(|_| ContextError::InvalidProxy)?,
        })
    }
}
