use windows::{
    core::{Interface, HSTRING},
    Data::Xml::Dom::XmlElement,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastNotification, ToastNotificationManager, ToastTemplateType},
};
use windows_sys::Win32::System::Registry::*;

static ACTIVE_TOASTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, ToastNotification>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

struct RegistryKey(HKEY);
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

pub(crate) fn register_identity(app_id: &str, icon: &std::path::Path) -> Result<(), String> {
    let name: Vec<u16> = format!(r"Software\Classes\AppUserModelId\{app_id}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut raw = std::ptr::null_mut();
    // Registry boundary: register this user's unpackaged app identity, without borrowing another application's AUMID.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut raw,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!("AUMID registration failed: {status}"));
    }
    let key = RegistryKey(raw);
    for (name, value) in [
        ("DisplayName", "TailSync".to_string()),
        ("IconUri", icon.to_string_lossy().into_owned()),
    ] {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let status = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        if status != 0 {
            return Err(format!("AUMID property write failed: {status}"));
        }
    }
    Ok(())
}

pub(crate) fn show(
    app_id: &str,
    key: &str,
    body: &str,
    passive: bool,
    on_activated: impl Fn() + Send + Sync + 'static,
) -> windows::core::Result<()> {
    let document = ToastNotificationManager::GetTemplateContent(ToastTemplateType::ToastText02)?;
    let texts = document.GetElementsByTagName(&HSTRING::from("text"))?;
    // OS/XML boundary: append text nodes so device names cannot inject Toast XML.
    texts
        .Item(0)?
        .AppendChild(&document.CreateTextNode(&HSTRING::from("TailSync"))?)?;
    texts
        .Item(1)?
        .AppendChild(&document.CreateTextNode(&HSTRING::from(body))?)?;
    let audio = document.CreateElement(&HSTRING::from("audio"))?;
    audio.SetAttribute(&HSTRING::from("silent"), &HSTRING::from("true"))?;
    document
        .GetElementsByTagName(&HSTRING::from("toast"))?
        .Item(0)?
        .cast::<XmlElement>()?
        .AppendChild(&audio)?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    // Windows limits Tag to 16 characters. Group+Tag replaces only this source or problem category.
    let tag = blake3::hash(key.as_bytes()).to_hex()[..16].to_string();
    toast.SetTag(&HSTRING::from(tag))?;
    toast.SetGroup(&HSTRING::from("TailSync"))?;
    toast.SetSuppressPopup(passive)?;
    let failed_key = key.to_string();
    toast.Failed(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<'_, windows::UI::Notifications::ToastFailedEventArgs>| {
            match args.as_ref() {
                Some(args) => match args.ErrorCode() {
                    Ok(code) => log::warn!(
                        "Native notification delivery failed key={failed_key} code={code:?}"
                    ),
                    Err(error) => log::warn!(
                        "Native notification failure status unreadable key={failed_key}: {error}"
                    ),
                },
                None => log::warn!("Native notification failed without status key={failed_key}"),
            }
            Ok(())
        },
    ))?;
    toast.Activated(&TypedEventHandler::new(move |_, _| {
        on_activated();
        Ok(())
    }))?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
    notifier.Show(&toast)?;
    // OS callback lifetime: retain the current Toast object for each replacement key until replacement or process exit.
    ACTIVE_TOASTS
        .lock()
        .expect("native notification handles poisoned")
        .insert(key.into(), toast);
    Ok(())
}
