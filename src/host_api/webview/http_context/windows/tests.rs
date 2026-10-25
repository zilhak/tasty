#[path = "tests/native_probe.rs"]
mod native_probe;
use super::*;
use std::{fmt, io::Write};
use windows::Win32::System::Com::{APTTYPE, APTTYPEQUALIFIER, CoGetApartmentType};

fn record(message: fmt::Arguments<'_>) {
    let path = std::env::var_os("U6_LOG").expect("explicit owned diagnostic log");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap();
    writeln!(file, "{message}").unwrap();
}
fn apartment() -> windows::core::Result<()> {
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    // SAFETY: native writes two correctly typed, live output slots on this thread.
    unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }
}
#[test]
fn unavailable_factory_does_not_initialize_a_native_apartment() {
    let before = apartment().unwrap_err().code();
    assert!(matches!(
        DataStore::new(),
        Err(ContextError::AuthenticatedSocksUnavailable)
    ));
    assert_eq!(apartment().unwrap_err().code(), before);
}
#[test]
#[ignore = "requires an owned Windows native fixture with U6_PROFILE and U6_LOG"]
fn hidden_webview_reports_actual_socks_authentication_boundary() -> Result<(), String> {
    assert!(matches!(
        DataStore::new(),
        Err(ContextError::AuthenticatedSocksUnavailable)
    ));
    let result = native_probe::run();
    if let Err(error) = &result {
        record(format_args!("NATIVE_OBSTACLE {error}"));
    }
    result
}
