//! Windows Jump List integration.
//!
//! Registers a "New Window" task in the taskbar Jump List.
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

/// PKEY_Title: {F29F85E0-4FF9-1068-AB91-08002B27B3D9}, 2
const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
    pid: 2,
};

/// Set up the Windows taskbar Jump List with a "New Window" task.
pub fn setup_jump_list() {
    if let Err(e) = setup_jump_list_inner() {
        tracing::warn!("Failed to set up jump list: {e}");
    }
}

fn setup_jump_list_inner() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        // Get the current executable path
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("current_exe failed: {e}"))?;
        let exe_wide: Vec<u16> = exe_path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // Create IShellLink for "New Window"
        let shell_link: IShellLinkW =
            windows::Win32::System::Com::CoCreateInstance(&ShellLink, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER)?;

        shell_link.SetPath(PCWSTR(exe_wide.as_ptr()))?;
        shell_link.SetArguments(w!("new-window"))?;
        shell_link.SetDescription(w!("Open a new Tasty window"))?;
        shell_link.SetIconLocation(PCWSTR(exe_wide.as_ptr()), 0)?;

        // Set PKEY_Title via IPropertyStore
        let property_store: IPropertyStore = shell_link.cast()?;
        let title_str = w!("New Window");
        let title_pcwstr = PCWSTR(title_str.as_ptr());
        let propvar = InitPropVariantFromStringVector(Some(&[title_pcwstr]))?;
        property_store.SetValue(&PKEY_TITLE, &propvar)?;
        property_store.Commit()?;

        // Create collection and add the shell link
        let collection: IObjectCollection =
            windows::Win32::System::Com::CoCreateInstance(&EnumerableObjectCollection, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER)?;
        collection.AddObject(&shell_link)?;

        // Create the Jump List
        let dest_list: ICustomDestinationList =
            windows::Win32::System::Com::CoCreateInstance(&DestinationList, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER)?;

        let mut min_slots: u32 = 0;
        let _removed: windows::Win32::UI::Shell::Common::IObjectArray =
            dest_list.BeginList(&mut min_slots)?;

        dest_list.AddUserTasks(&collection)?;
        dest_list.CommitList()?;

        tracing::info!("Jump list set up successfully");
    }

    Ok(())
}
