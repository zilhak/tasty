//! Windows Jump List integration.
//!
//! Registers a "New Window" task in the taskbar Jump List. The task title and
//! description are user-facing strings and come from `t("jump_list.*")`
//! (`docs/dev-guide/i18n.md`); only the `new-window` launch argument stays a
//! fixed token. `w!()` is a compile-time literal, so translated strings are
//! converted to null-terminated UTF-16 at runtime instead.
//! Only compiled on Windows.

#![cfg(windows)]

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::InitPropVariantFromStringVector;
use windows::Win32::UI::Shell::Common::IObjectCollection;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
};
use windows::core::{Interface, PCWSTR, w};

use crate::i18n::t;

/// PKEY_Title: {F29F85E0-4FF9-1068-AB91-08002B27B3D9}, 2
const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
    pid: 2,
};

/// Null-terminated UTF-16 buffer for a runtime string, for use as `PCWSTR`.
/// The returned `Vec` must outlive every `PCWSTR` pointing into it.
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Set up the Windows taskbar Jump List with a "New Window" task.
pub fn setup_jump_list() {
    if let Err(e) = setup_jump_list_inner() {
        tracing::warn!("Failed to set up jump list: {e}");
    }
}

fn setup_jump_list_inner() -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: 전체 Windows Jump List COM 시퀀스를 단일 thread (main)에서 실행.
    // CoCreateInstance는 process-wide COM init 후 호출되며(main.rs에서 보장),
    // 모든 PCWSTR 인자는 'static w!() 매크로 또는 호출 끝까지 살아있는
    // local Vec<u16>(`wide_null`)의 ptr이다 — 번역 라벨(desc_wide / title_wide)도
    // 이 블록이 끝날 때까지 drop 되지 않는다. shell_link.cast()는 같은 객체의 COM interface
    // re-query로 safe. IObjectArray/ICustomDestinationList 등 모든 호출은
    // 같은 thread에서 순차 실행되며 COM 객체는 Drop으로 자동 Release.
    unsafe {
        // Get the current executable path
        let exe_path = std::env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?;
        let exe_wide = wide_null(&exe_path.to_string_lossy());

        // Create IShellLink for "New Window"
        let shell_link: IShellLinkW = windows::Win32::System::Com::CoCreateInstance(
            &ShellLink,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )?;

        shell_link.SetPath(PCWSTR(exe_wide.as_ptr()))?;
        shell_link.SetArguments(w!("new-window"))?;
        let desc_wide = wide_null(t("jump_list.new_window_desc"));
        shell_link.SetDescription(PCWSTR(desc_wide.as_ptr()))?;
        shell_link.SetIconLocation(PCWSTR(exe_wide.as_ptr()), 0)?;

        // Set PKEY_Title via IPropertyStore
        let property_store: IPropertyStore = shell_link.cast()?;
        let title_wide = wide_null(t("jump_list.new_window"));
        let title_pcwstr = PCWSTR(title_wide.as_ptr());
        let propvar = InitPropVariantFromStringVector(Some(&[title_pcwstr]))?;
        property_store.SetValue(&PKEY_TITLE, &propvar)?;
        property_store.Commit()?;

        // Create collection and add the shell link
        let collection: IObjectCollection = windows::Win32::System::Com::CoCreateInstance(
            &EnumerableObjectCollection,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )?;
        collection.AddObject(&shell_link)?;

        // Create the Jump List
        let dest_list: ICustomDestinationList = windows::Win32::System::Com::CoCreateInstance(
            &DestinationList,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )?;

        let mut min_slots: u32 = 0;
        let _removed: windows::Win32::UI::Shell::Common::IObjectArray =
            dest_list.BeginList(&mut min_slots)?;

        dest_list.AddUserTasks(&collection)?;
        dest_list.CommitList()?;

        tracing::info!("Jump list set up successfully");
    }

    Ok(())
}
