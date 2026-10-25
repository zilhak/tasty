# Windows remote HTTP context boundary

The Windows remote HTTP context factory refuses construction with
`ContextError::AuthenticatedSocksUnavailable`. Its `DataStore` has an uninhabited
field, so no successful store or native view can be returned. The check performs
no filesystem, COM, network, runtime or protocol-ID work. The shared host must
check this boundary before preparing a profile or endpoint and retain its
unsupported remote state. It must not substitute the normal local WebView.
This component is source-bound in its fixture; the shared native module declaration
and App error presentation remain the integrating host's responsibility.

## Required transport and native evidence

The document endpoint and its private TLS probe require SOCKS5 username/password
authentication with unpredictable endpoint credentials. Chromium's
[proxy contract](https://github.com/chromium/chromium/blob/main/net/docs/proxy.md#socksv5-proxy-scheme)
does not support SOCKS5 authentication. Browser arguments and the WebView2 HTTP
BasicAuthenticationRequested event do not establish RFC1929 authentication.
Removing endpoint authentication, forwarding through an unauthenticated local
relay or enabling a direct route would change the required boundary.

The pinned `webview2-com` binding is0.38.2 with `windows`0.61.3. A source-bound
hidden native probe created WebView2 environment155.0.4283.62 from SSH session0.
Controller creation returned0x80070578 (invalid window handle). A registered,
verified-live hidden HWND and InPrivate controller options gave the same error.
The probe did not reach navigation, SOCKS, HTTP or TLS. This is an observed
controller obstacle, not a native measurement of authentication support or a
claim that every Windows session has this restriction. No interactive desktop
or input was used.

WebView2's
[CertificateIsInvalid status](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2weberrorstatus)
includes unknown authorities as well as weak algorithms/keys, pin failures and
other errors. That value alone cannot attest unknown-CA-only. Matching a leaf
hash does not narrow the native cause. The Windows factory never grants a
certificate exception or acknowledges an endpoint NativeTrust request.

## Ownership and integration

No Windows target profile, native probe pool or certificate history is allocated
by the refused factory. A future supported native implementation must use one
separate process-local, permanently untrusted private probe resource across
sessions and Reloads through the endpoint ProbePool contract. It must preserve
the exact target authority, leaf, current view/source, original native request
and upload, enforce finite target trust history, and clear selected HTTP cache
and service-worker data while retaining cookies/storage and the same view.

Those properties, native cookie isolation, loopback/WebSocket containment,
same-view POST continuation, exact scan/render bytes and App capability
negotiation are not implemented by this refusal boundary. HTTP/HTTPS capability
activation remains off. The existing file-entry response adapter is separate
and is not changed by this module.

## Reproducing the component checks

The source-bound fixture builder in
`src/host_api/webview/http_context/windows/tests/fixture.py` writes an isolated
Cargo package referencing the actual factory and its test modules. It extracts
the exact registry dependency closure from the root lock; it does not resolve
new versions or modify the root manifest/lock. Use the existing owned target.

The ordinary Windows test confirms that factory refusal leaves the native COM
apartment uninitialized. The explicitly ignored native test additionally needs
owned absolute `U6_PROFILE` and `U6_LOG` paths. Run that test alone, with one test
thread, in an authorized Windows environment. It creates only a hidden window,
uses local diagnostic SOCKS/target sentinels, disables scripts and bounds each
callback wait at20seconds. It records actual API errors; an environment failure
is a failed native test, never a successful transport observation. Preserve its
profile while native descendants remain uncertain. Native process ownership
and cleanup belong to the caller; no user desktop or input is required.
