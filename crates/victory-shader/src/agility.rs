//! The Microsoft DirectX 12 Agility SDK (`D3D12Core.dll`), loaded at run
//! time. Written for Projeto Lucia from Microsoft's documented API
//! (GPL-3.0-or-later, like the rest of this crate).
//!
//! Windows 10's own D3D12 runtime stops at Shader Model 6.5 even on cards
//! whose driver offers more: a GTX 1650 SUPER reports 6.5 through it and
//! 6.8 through Agility 1.620. Translated shaders need 6.6 (directly indexed
//! heaps), so devices are made through
//! `ID3D12SDKConfiguration1::CreateDeviceFactory` (Windows 10 19045.3393+
//! and Windows 11), which needs neither Developer Mode nor a
//! `D3D12SDKVersion` export from the executable.
//!
//! The folder is the first of these that holds a `D3D12Core.dll`
//! exporting `D3D12SDKVersion`: `VICTORY_D3D12_DIR`, `<exe dir>\D3D12`,
//! `<exe dir>`. `VICTORY_D3D12_DIR=off` keeps the inbox runtime. Without
//! a usable folder everything falls back to the inbox runtime, as before.
//!
//! `CreateDeviceFactory` refused a folder whose path had a space in it
//! (`0x887E0003` on Windows 10 19045.3803) and took the same folder by a
//! path without one, so a folder is handed over by its short (8.3) name
//! first.

use std::ffi::CString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{GUID, IUnknown, Interface, PCSTR};

/// The environment variable naming the Agility SDK folder (`off`: none).
pub const ENV: &str = "VICTORY_D3D12_DIR";

/// What this process makes its D3D12 devices with.
pub enum State {
    /// The Agility SDK, through its device factory.
    Loaded {
        factory: ID3D12DeviceFactory,
        /// Its `D3D12SDKVersion` export.
        version: u32,
        dir: PathBuf,
    },
    /// The runtime that comes with Windows, and why.
    Inbox { why: String },
}

/// The state, kept for the whole process.
struct Kept(State);

// SAFETY: the device factory is only used to make devices and read
// configuration interfaces, both free-threaded in D3D12; it is never
// released before the process ends.
unsafe impl Send for Kept {}
// SAFETY: as above.
unsafe impl Sync for Kept {}

/// The state, found on first use.
pub fn state() -> &'static State {
    static STATE: OnceLock<Kept> = OnceLock::new();
    &STATE.get_or_init(|| Kept(load())).0
}

/// One line for the log.
pub fn describe() -> String {
    match state() {
        State::Loaded { version, dir, .. } => {
            format!("Agility SDK {version} from {}", dir.display())
        }
        State::Inbox { why } => format!("inbox D3D12 runtime ({why})"),
    }
}

/// A device on `adapter` (the default adapter for `None`), from the Agility
/// SDK when it loaded and from the inbox runtime otherwise.
///
/// # Safety
/// As `D3D12CreateDevice`: `adapter` is a live DXGI adapter.
pub unsafe fn create_device(
    adapter: Option<&IUnknown>,
    level: D3D_FEATURE_LEVEL,
) -> windows::core::Result<ID3D12Device> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: as the caller promises; `device` is a valid out slot.
    unsafe {
        match state() {
            State::Loaded { factory, .. } => factory.CreateDevice(adapter, level, &mut device)?,
            State::Inbox { .. } => D3D12CreateDevice(adapter, level, &mut device)?,
        }
    }
    device.ok_or_else(windows::core::Error::empty)
}

/// A debug or device-removed-data configuration interface (`clsid` is
/// `CLSID_D3D12Debug` or `CLSID_D3D12DeviceRemovedExtendedData`) from the
/// same runtime the devices come from. It must be set before the device is
/// made.
///
/// # Safety
/// Plain COM: `T` is the interface `clsid` gives.
pub unsafe fn configuration<T: Interface>(clsid: &GUID) -> windows::core::Result<T> {
    // SAFETY: as the caller promises.
    unsafe {
        match state() {
            State::Loaded { factory, .. } => factory.GetConfigurationInterface(clsid),
            State::Inbox { .. } => {
                let mut out: Option<T> = None;
                D3D12GetDebugInterface(&mut out)?;
                out.ok_or_else(windows::core::Error::empty)
            }
        }
    }
}

/// Whether devices come from the Agility SDK.
pub fn loaded() -> bool {
    matches!(state(), State::Loaded { .. })
}

/// Serializes a root signature with the runtime `device` comes from.
///
/// `D3D12SerializeVersionedRootSignature` is an export of the inbox
/// `d3d12.dll` and serializes with the inbox runtime even when devices come
/// from the Agility SDK: on Windows 10 it refuses the Shader Model 6.6 flags
/// (`*_HEAP_DIRECTLY_INDEXED`) with `E_INVALIDARG`. A device's
/// `ID3D12DeviceConfiguration` serializes with its own runtime; the export
/// is used when there is no such device or no Agility SDK.
///
/// # Safety
/// As `D3D12SerializeVersionedRootSignature`: `desc` is a valid description
/// and the out slots are writable.
pub unsafe fn serialize_root_signature(
    device: Option<&ID3D12Device>,
    desc: *const D3D12_VERSIONED_ROOT_SIGNATURE_DESC,
    blob: *mut Option<ID3DBlob>,
    error: Option<*mut Option<ID3DBlob>>,
) -> windows::core::Result<()> {
    // SAFETY: as the caller promises.
    unsafe {
        if loaded()
            && let Some(config) =
                device.and_then(|d| d.cast::<ID3D12DeviceConfiguration>().ok())
        {
            return config.SerializeVersionedRootSignature(desc, blob, error);
        }
        D3D12SerializeVersionedRootSignature(desc, blob, error)
    }
}

fn load() -> State {
    let set = std::env::var_os(ENV);
    if set.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("off")) {
        return State::Inbox {
            why: format!("{ENV}=off"),
        };
    }
    let mut dirs = Vec::new();
    if let Some(dir) = set.filter(|s| !s.is_empty()) {
        dirs.push(std::path::absolute(&dir).unwrap_or_else(|_| PathBuf::from(dir)));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("D3D12"));
        dirs.push(dir.to_path_buf());
    }
    let mut refused = Vec::new();
    for dir in dirs {
        let Some(version) = sdk_version(&dir.join("D3D12Core.dll")) else {
            continue;
        };
        match open(&dir, version) {
            Ok(factory) => {
                return State::Loaded {
                    factory,
                    version,
                    dir,
                };
            }
            Err(e) => refused.push(format!("{}: {e}", dir.display())),
        }
    }
    let why = if refused.is_empty() {
        format!("no D3D12Core.dll with D3D12SDKVersion in {ENV}, <exe>\\D3D12 or <exe>")
    } else {
        format!("refused {}", refused.join("; "))
    };
    State::Inbox { why }
}

/// The `D3D12SDKVersion` a `D3D12Core.dll` exports; `None` if `dll` is
/// missing, of another architecture or exports none.
pub fn sdk_version(dll: &Path) -> Option<u32> {
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{
        DONT_RESOLVE_DLL_REFERENCES, GetProcAddress, LoadLibraryExW,
    };
    if !dll.is_file() {
        return None;
    }
    let wide = wide(dll);
    // SAFETY: a NUL-terminated path; the module is only read (its imports
    // are not resolved and its entry point does not run), then freed.
    unsafe {
        let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), DONT_RESOLVE_DLL_REFERENCES);
        if module.is_null() {
            return None;
        }
        let export = GetProcAddress(module, c"D3D12SDKVersion".as_ptr().cast());
        let version = export.map(|f| *(f as *const u32));
        FreeLibrary(module);
        version
    }
}

/// The device factory of the Agility SDK in `dir`.
fn open(dir: &Path, version: u32) -> windows::core::Result<ID3D12DeviceFactory> {
    // SAFETY: plain COM calls with valid out slots and NUL-terminated paths.
    unsafe {
        let mut config: Option<ID3D12SDKConfiguration1> = None;
        D3D12GetInterface(&CLSID_D3D12SDKConfiguration, &mut config)?;
        let config = config.ok_or_else(windows::core::Error::empty)?;
        let mut last = windows::core::Error::empty();
        for path in arguments(dir) {
            match config.CreateDeviceFactory(version, PCSTR(path.as_ptr().cast())) {
                Ok(factory) => return Ok(factory),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

/// The ways to name `dir` to `CreateDeviceFactory`, best first: its short
/// name (no spaces), then the full path; each ends in a backslash.
fn arguments(dir: &Path) -> Vec<CString> {
    let mut out = Vec::new();
    for path in [short(dir), Some(dir.to_path_buf())].into_iter().flatten() {
        let mut text = path.to_string_lossy().into_owned();
        if !text.ends_with('\\') {
            text.push('\\');
        }
        if let Ok(c) = CString::new(text)
            && !out.contains(&c)
        {
            out.push(c);
        }
    }
    out
}

/// `path` by its short (8.3) name; `None` where the volume keeps none.
pub fn short(path: &Path) -> Option<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide = wide(path);
    let mut buffer = vec![0u16; 1024];
    // SAFETY: a NUL-terminated path and a buffer of the length given.
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
    if len == 0 || len as usize >= buffer.len() {
        return None;
    }
    let short = String::from_utf16(&buffer[..len as usize]).ok()?;
    (!short.contains(' ')).then(|| PathBuf::from(short))
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_version_without_a_dll() {
        assert_eq!(sdk_version(Path::new("C:\\no\\such\\folder\\D3D12Core.dll")), None);
    }

    /// The folder `VICTORY_D3D12_DIR` names holds a `D3D12Core.dll` whose
    /// SDK version reads back (1.620 exports 620).
    #[test]
    fn a_named_agility_sdk_has_a_version() {
        let Some(dir) = std::env::var_os(ENV).filter(|s| !s.eq_ignore_ascii_case("off")) else {
            eprintln!("{ENV} not set; skipped");
            return;
        };
        let version = sdk_version(&Path::new(&dir).join("D3D12Core.dll"));
        assert!(version.is_some_and(|v| v >= 600), "{version:?}");
    }

    #[test]
    fn arguments_end_in_a_backslash_and_short_names_have_no_space() {
        let dir = std::env::temp_dir().join(format!("lucia agility {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let args = arguments(&dir);
        assert!(!args.is_empty());
        for a in &args {
            assert!(a.to_bytes().ends_with(b"\\"));
        }
        if let Some(s) = short(&dir) {
            assert!(!s.to_string_lossy().contains(' '));
            assert_eq!(args[0].to_str().unwrap().trim_end_matches('\\'), s.to_str().unwrap());
        }
        let _ = std::fs::remove_dir(&dir);
    }

    /// With `VICTORY_D3D12_DIR` naming an Agility SDK, devices come from it
    /// and give Shader Model 6.6 or later.
    #[test]
    fn a_named_agility_sdk_loads_and_gives_shader_model_6_6() {
        if std::env::var_os(ENV).is_none_or(|s| s.eq_ignore_ascii_case("off")) {
            eprintln!("{ENV} not set; skipped");
            return;
        }
        assert!(matches!(state(), State::Loaded { .. }), "{}", describe());
        // SAFETY: the default adapter; a valid out slot.
        unsafe {
            let Ok(device) = create_device(None, windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0) else {
                eprintln!("no hardware device; skipped");
                return;
            };
            let mut sm = D3D12_FEATURE_DATA_SHADER_MODEL {
                HighestShaderModel: D3D_SHADER_MODEL_6_6,
            };
            device
                .CheckFeatureSupport(
                    D3D12_FEATURE_SHADER_MODEL,
                    (&raw mut sm).cast(),
                    size_of_val(&sm) as u32,
                )
                .unwrap();
            assert!(sm.HighestShaderModel.0 >= D3D_SHADER_MODEL_6_6.0);
        }
    }
}
