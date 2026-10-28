//! WebView2 context for a remote HTTP document. Every request goes through the document
//! endpoint as an authenticated HTTP proxy, including loopback hosts; no direct route exists.
use super::super::http_native::NativeDocument;
use crate::app::http_session::HttpSession;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::{ffi::c_void, rc::Weak};
use tasty_remote::html_http_client::ReceiveState;
use webview2_com::{
    BasicAuthenticationRequestedEventHandler, CoreWebView2EnvironmentOptions,
    Microsoft::Web::WebView2::Win32::*, ServerCertificateErrorDetectedEventHandler,
};
use windows::{Win32::System::Com::CoTaskMemFree, core::*};

pub(in crate::host_api::webview) struct HttpEnvironment {
    pub options: ICoreWebView2EnvironmentOptions,
    pub profile: tempfile::TempDir,
    pub folder: Vec<u16>,
}
impl HttpEnvironment {
    /// A private user-data folder keeps this browser process, its proxy and its cookies apart
    /// from local HTML views and other connections.
    pub fn new(document: &NativeDocument) -> std::io::Result<Self> {
        let root = tasty_utils::path::tasty_home()
            .ok_or_else(|| std::io::Error::other("Tasty home is unavailable"))?
            .join("webview/http");
        std::fs::create_dir_all(&root)?;
        let profile = tempfile::Builder::new().prefix("http-").tempdir_in(root)?;
        use std::os::windows::ffi::OsStrExt;
        let folder = profile
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // `<-loopback>` removes Chromium's implicit loopback bypass, so a remote localhost
        // address also goes to the remote host. QUIC would not use the proxy.
        let arguments = format!(
            "--proxy-server={} --proxy-bypass-list=<-loopback> --disable-quic --disable-background-networking",
            document.credentials.uri.trim_end_matches('/')
        );
        let arguments: Vec<u16> = arguments.encode_utf16().chain(Some(0)).collect();
        let options: ICoreWebView2EnvironmentOptions =
            CoreWebView2EnvironmentOptions::default().into();
        // SAFETY: the freshly owned options object has not escaped this thread; the argument
        // buffer is NUL-terminated and outlives the call.
        unsafe { options.SetAdditionalBrowserArguments(PCWSTR(arguments.as_ptr())) }?;
        Ok(Self {
            options,
            profile,
            folder,
        })
    }
}

fn take_string(value: PWSTR) -> String {
    // SAFETY: WebView2 returned a NUL-terminated string the caller owns and frees here.
    let text = unsafe { value.to_string() }.unwrap_or_default();
    // SAFETY: the caller owns this CoTaskMem allocation.
    unsafe { CoTaskMemFree(Some(value.0 as *const c_void)) };
    text
}

/// Proxy credentials go only to this view's own endpoint challenge. A remote page's 401, even on
/// the same loopback address, carries a different realm and is cancelled.
pub(in crate::host_api::webview) fn install(
    webview: &ICoreWebView2,
    surface_id: u32,
    document: Weak<NativeDocument>,
) -> Result<()> {
    let mut token = 0;
    let auth_document = document.clone();
    let auth = BasicAuthenticationRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut uri = PWSTR::null();
        let mut challenge = PWSTR::null();
        // SAFETY: live event args on the WebView2 apartment; outputs are owned and freed here.
        unsafe {
            args.Uri(&mut uri)?;
            args.Challenge(&mut challenge)?;
        }
        let (uri, challenge) = (take_string(uri), take_string(challenge));
        let Some(document) = auth_document
            .upgrade()
            .filter(|document| document.session.is_current())
            .filter(|document| {
                uri == document.credentials.uri && challenge == document.credentials.challenge
            })
        else {
            tracing::debug!(
                "WebView surface {surface_id}: HTTP authentication challenge cancelled"
            );
            // SAFETY: live event args on the WebView2 apartment.
            return unsafe { args.SetCancel(true) };
        };
        let user: Vec<u16> = document
            .credentials
            .user
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let password: Vec<u16> = document
            .credentials
            .password
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: live response object; both buffers are NUL-terminated for the calls.
        unsafe {
            let response = args.Response()?;
            response.SetUserName(PCWSTR(user.as_ptr()))?;
            response.SetPassword(PCWSTR(password.as_ptr()))
        }
    }));
    let view10: ICoreWebView2_10 = webview.cast()?;
    // SAFETY: live WebView and handler on the COM apartment thread. The WebView owns the handler.
    unsafe { view10.add_BasicAuthenticationRequested(&auth, &mut token) }?;

    let certificate =
        ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let allow = endpoint_leaf(&args, &document, surface_id).unwrap_or_else(|error| {
                tracing::warn!(
                    "WebView surface {surface_id}: certificate inspection failed: {error}"
                );
                false
            });
            let action = if allow {
                COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW
            } else {
                COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL
            };
            // SAFETY: live event args on the WebView2 apartment.
            unsafe { args.SetAction(action) }
        }));
    let view14: ICoreWebView2_14 = webview.cast()?;
    // SAFETY: live WebView and handler on the COM apartment thread. The WebView owns the handler.
    unsafe { view14.add_ServerCertificateErrorDetected(&certificate, &mut token) }?;

    // SAFETY: live settings object on the WebView2 apartment.
    unsafe {
        let settings = webview.Settings()?;
        // Reputation checks would contact a Microsoft service through the endpoint, which
        // refuses them; autofill would retain remote form data in this profile.
        settings
            .cast::<ICoreWebView2Settings8>()?
            .SetIsReputationCheckingRequired(false)?;
        let settings4 = settings.cast::<ICoreWebView2Settings4>()?;
        settings4.SetIsPasswordAutosaveEnabled(false)?;
        settings4.SetIsGeneralAutofillEnabled(false)?;
    }
    super::entry_adapter::deny_new_windows(webview)
}

/// Only the exact leaf this endpoint generated for the authority, failing only as an untrusted
/// certificate, is accepted. Any other defect or certificate keeps WebView2's refusal.
fn endpoint_leaf(
    args: &ICoreWebView2ServerCertificateErrorDetectedEventArgs,
    document: &Weak<NativeDocument>,
    surface_id: u32,
) -> Result<bool> {
    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
    let mut uri = PWSTR::null();
    let mut pem = PWSTR::null();
    // SAFETY: live event args and certificate on the WebView2 apartment; outputs are freed.
    unsafe {
        args.ErrorStatus(&mut status)?;
        args.RequestUri(&mut uri)?;
        args.ServerCertificate()?.ToPemEncoding(&mut pem)?;
    }
    let (uri, pem) = (take_string(uri), take_string(pem));
    let Some(der) = pem_der(&pem) else {
        return Ok(false);
    };
    let leaf: [u8; 32] = Sha256::digest(&der).into();
    let Some((host, port)) = url::Url::parse(&uri).ok().and_then(|url| {
        let host = match url.host()? {
            url::Host::Domain(name) => name.to_owned(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => ip.to_string(),
        };
        Some((host, url.port_or_known_default()?))
    }) else {
        return Ok(false);
    };
    let expected = document
        .upgrade()
        .and_then(|document| document.expected_leaf(&host, port));
    let allow =
        status == COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID && expected == Some(leaf);
    tracing::debug!(
        "WebView surface {surface_id}: certificate error status={} for {host}:{port}; endpoint leaf {}",
        status.0,
        if allow { "accepted" } else { "refused" }
    );
    Ok(allow)
}

fn pem_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

/// A deliberate skip or cancel of the remote entry ends the load without failing the session.
pub(in crate::host_api::webview) fn load_failed(session: &HttpSession) {
    let intentional = session.bridge.document().is_some_and(|document| {
        document
            .with_current(|model| {
                matches!(
                    model.state(document.entry()),
                    Some(ReceiveState::Skipped | ReceiveState::Cancelled)
                )
            })
            .unwrap_or(false)
    });
    if session.is_current() && !intentional {
        session.fail();
    }
}

pub(in crate::host_api::webview) fn proxy_challenge(
    args: &ICoreWebView2NavigationCompletedEventArgs,
) -> bool {
    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
    // SAFETY: live event args on the WebView2 apartment.
    unsafe { args.WebErrorStatus(&mut status) }.is_ok()
        && status == COREWEBVIEW2_WEB_ERROR_STATUS_VALID_PROXY_AUTHENTICATION_REQUIRED
}
