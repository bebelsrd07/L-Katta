//! Appels système Windows partagés (sans dépendance externe).
//!
//! Chaque bloc `extern "system"` parle directement à une DLL fournie avec Windows 10 et 11
//! (`kernel32`, `user32`, `gdi32`, `advapi32`, `shell32`, `ntdll`, `bcrypt`). Tout `unsafe` reste
//! enfermé ici derrière des fonctions Rust ordinaires, bornées et documentées.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, OnceLock};

// ── Structures ABI Windows (x86_64) ──────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
struct MonitorInfoExW {
    cb_size: u32,
    rc_monitor: Rect,
    rc_work: Rect,
    dw_flags: u32,
    sz_device: [u16; 32],
}

#[repr(C)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[repr(C)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[repr(C)]
struct OsVersionInfoW {
    dw_os_version_info_size: u32,
    dw_major_version: u32,
    dw_minor_version: u32,
    dw_build_number: u32,
    dw_platform_id: u32,
    sz_csd_version: [u16; 128],
}

#[repr(C)]
struct LastInputInfo {
    cb_size: u32,
    dw_time: u32,
}

#[repr(C)]
struct CredentialW {
    flags: u32,
    typ: u32,
    target_name: *mut u16,
    comment: *mut u16,
    last_written: [u32; 2],
    credential_blob_size: u32,
    credential_blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut c_void,
    target_alias: *mut u16,
    user_name: *mut u16,
}

#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
struct IconInfo {
    f_icon: i32,
    x_hotspot: u32,
    y_hotspot: u32,
    hbm_mask: isize,
    hbm_color: isize,
}

type EnumMonProc = unsafe extern "system" fn(hmon: isize, hdc: isize, rect: *const Rect, data: isize) -> i32;
type GetDpiForMonitorFn = unsafe extern "system" fn(hmon: isize, typ: i32, dpi_x: *mut u32, dpi_y: *mut u32) -> i32;

// ── Liaisons système ─────────────────────────────────────────────────────────

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(pid: u32) -> i32;
    fn GetStdHandle(which: u32) -> isize;
    fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
    fn SetConsoleMode(h: isize, mode: u32) -> i32;
    fn GetCurrentThread() -> isize;
    fn SetThreadPriority(thread: isize, prio: i32) -> i32;
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    fn GlobalMemoryStatusEx(stat: *mut MemoryStatusEx) -> i32;
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    fn GetUserDefaultLocaleName(name: *mut u16, cch: i32) -> i32;
    fn GetTickCount() -> u32;
    fn GetTickCount64() -> u64;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
    fn EnumDisplayMonitors(hdc: isize, clip: *const c_void, cb: EnumMonProc, data: isize) -> i32;
    fn GetMonitorInfoW(hmon: isize, info: *mut c_void) -> i32;
    fn MonitorFromPoint(pt: Point, flags: u32) -> isize;
    fn GetCursorPos(pt: *mut Point) -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    fn GetDeviceCaps(hdc: isize, index: i32) -> i32;
    fn CreateIconIndirect(info: *const IconInfo) -> isize;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: isize) -> isize;
    fn CreateDIBSection(
        hdc: isize,
        info: *const BitmapInfoHeader,
        usage: u32,
        bits: *mut *mut c_void,
        section: isize,
        offset: u32,
    ) -> isize;
    fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const c_void) -> isize;
    fn SelectObject(hdc: isize, obj: isize) -> isize;
    fn BitBlt(
        hdc_dst: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        hdc_src: isize,
        x1: i32,
        y1: i32,
        rop: u32,
    ) -> i32;
    fn DeleteDC(hdc: isize) -> i32;
    fn DeleteObject(obj: isize) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        hkey: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        typ: *mut u32,
        data: *mut c_void,
        cb_data: *mut u32,
    ) -> i32;
    fn RegOpenKeyExW(hkey: isize, subkey: *const u16, options: u32, sam: u32, out: *mut isize) -> i32;
    fn RegCreateKeyExW(
        hkey: isize,
        subkey: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        sam: u32,
        sec: *const c_void,
        out: *mut isize,
        disp: *mut u32,
    ) -> i32;
    fn RegSetValueExW(hkey: isize, value: *const u16, reserved: u32, typ: u32, data: *const u8, cb: u32) -> i32;
    fn RegDeleteValueW(hkey: isize, value: *const u16) -> i32;
    fn RegEnumKeyExW(
        hkey: isize,
        index: u32,
        name: *mut u16,
        cch_name: *mut u32,
        reserved: *mut u32,
        class: *mut u16,
        cch_class: *mut u32,
        last_write: *mut u64,
    ) -> i32;
    fn RegCloseKey(hkey: isize) -> i32;
    fn CredReadW(target: *const u16, typ: u32, flags: u32, cred: *mut *mut CredentialW) -> i32;
    fn CredWriteW(cred: *const CredentialW, flags: u32) -> i32;
    fn CredDeleteW(target: *const u16, typ: u32, flags: u32) -> i32;
    fn CredFree(buf: *mut c_void);
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHQueryUserNotificationState(state: *mut i32) -> i32;
    fn ShellExecuteW(
        hwnd: isize,
        op: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> isize;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptOpenAlgorithmProvider(alg: *mut isize, id: *const u16, imp: *const u16, flags: u32) -> i32;
    fn BCryptCloseAlgorithmProvider(alg: isize, flags: u32) -> i32;
    fn BCryptHash(
        alg: isize,
        secret: *const u8,
        secret_len: u32,
        input: *const u8,
        input_len: u32,
        output: *mut u8,
        output_len: u32,
    ) -> i32;
}

// ── Constantes ───────────────────────────────────────────────────────────────

pub const HKCU: isize = 0x8000_0001u32 as i32 as isize;
pub const HKLM: isize = 0x8000_0002u32 as i32 as isize;

const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const ENABLE_ECHO_INPUT: u32 = 0x0004;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

const THREAD_PRIORITY_LOWEST: i32 = -2;
const THREAD_MODE_BACKGROUND_BEGIN: i32 = 0x0001_0000;

const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const RRF_RT_REG_BINARY: u32 = 0x0000_0008;
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
const RRF_RT_REG_QWORD: u32 = 0x0000_0040;
const KEY_READ: u32 = 0x0002_0019;
const KEY_SET_VALUE: u32 = 0x0000_0002;
const REG_SZ: u32 = 1;

const CRED_TYPE_GENERIC: u32 = 1;
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const CRED_MAX_BLOB: usize = 2560;

const DRIVE_FIXED: u32 = 3;
const MONITOR_DEFAULTTONEAREST: u32 = 2;
const MONITORINFOF_PRIMARY: u32 = 1;
const LOGPIXELSX: i32 = 88;
const SRCCOPY: u32 = 0x00CC_0020;
const SW_SHOWNORMAL: i32 = 1;

/// Chaîne UTF-16 terminée par un NUL.
pub fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Chemin ou `OsStr` en UTF-16 terminé par un NUL (sans perte sur les noms non UTF-8).
pub fn large_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn depuis_utf16_nul(buf: &[u16]) -> String {
    let fin = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..fin])
}

// ── Console, DPI et priorité ─────────────────────────────────────────────────

/// Déclare le processus conscient de la densité par écran (Per-Monitor DPI v2).
///
/// Sans cet appel, Windows dessine L Katta en 96 ppp puis étire l'image sur les écrans à 125 %,
/// 150 % ou 200 % : le maki et le texte des bulles deviennent flous.
pub fn activer_dpi() {
    const PER_MONITOR_V2: isize = -4;
    const PER_MONITOR: isize = -3;
    // SAFETY : pseudo-poignées constantes définies par Windows 10 1703+ ; échec sans effet.
    unsafe {
        if SetProcessDpiAwarenessContext(PER_MONITOR_V2) == 0 {
            SetProcessDpiAwarenessContext(PER_MONITOR);
        }
    }
}

/// Rattache la sortie au terminal parent (PowerShell, CMD, Windows Terminal) quand `lkatta.exe` est
/// lancé en ligne de commande, sans écraser une sortie déjà redirigée vers un tube ou un fichier,
/// et sans jamais ouvrir de fenêtre noire sur un double-clic depuis l'Explorateur.
pub fn attacher_console() {
    // SAFETY : lecture de STD_OUTPUT_HANDLE ; s'il est déjà redirigé (tube, fichier, `maj.rs`), on
    // le garde tel quel, sinon `AttachConsole` remplacerait le tube par l'écran du terminal.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out == 0 || out == -1 {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// Empêche un sous-processus d'arrière-plan (`curl`, `git`, `powershell`…) d'ouvrir un bref cadre
/// noir à l'écran quand L Katta tourne sans console.
pub fn sans_fenetre(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.creation_flags(CREATE_NO_WINDOW)
}

/// Lit une ligne secrète sur l'entrée standard en coupant l'écho du terminal (remis même en cas
/// d'erreur).
pub fn lire_secret_console() -> Result<String, String> {
    struct GardeEcho(isize, u32);
    impl Drop for GardeEcho {
        fn drop(&mut self) {
            if self.0 != 0 && self.0 != -1 {
                // SAFETY : restaure le mode exact lu avant la saisie.
                unsafe { SetConsoleMode(self.0, self.1) };
            }
        }
    }
    // SAFETY : récupère la poignée d'entrée standard et son mode si c'est une console.
    let _garde = unsafe {
        let h = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0u32;
        if h != 0 && h != -1 && GetConsoleMode(h, &mut mode) != 0 {
            SetConsoleMode(h, mode & !ENABLE_ECHO_INPUT);
            Some(GardeEcho(h, mode))
        } else {
            None
        }
    };
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).map_err(|e| e.to_string())?;
    if _garde.is_some() {
        eprintln!();
    }
    Ok(s.trim_end_matches(['\r', '\n']).to_string())
}

/// Met le fil courant en priorité processeur et E/S d'arrière-plan (équivalent de nice 19 + ionice
/// idle sous Linux).
pub fn fil_discret() {
    // SAFETY : GetCurrentThread renvoie la pseudo-poignée du fil courant (-2), toujours valide.
    unsafe {
        let t = GetCurrentThread();
        SetThreadPriority(t, THREAD_PRIORITY_LOWEST);
        SetThreadPriority(t, THREAD_MODE_BACKGROUND_BEGIN);
    }
}

// ── Registre Windows ─────────────────────────────────────────────────────────

pub fn reg_chaine(racine: isize, cle: &str, valeur: &str) -> Option<String> {
    let cle = large(cle);
    let valeur = large(valeur);
    let mut octets = 0u32;
    // SAFETY : premier appel sans tampon pour obtenir la taille en octets.
    if unsafe { RegGetValueW(racine, cle.as_ptr(), valeur.as_ptr(), RRF_RT_REG_SZ, ptr::null_mut(), ptr::null_mut(), &mut octets) } != 0
        || octets < 2
        || octets > 16_384
    {
        return None;
    }
    let mut buf = vec![0u16; (octets as usize).div_ceil(2)];
    let mut cap = (buf.len() * 2) as u32;
    // SAFETY : `buf` fait au moins `octets` octets.
    if unsafe { RegGetValueW(racine, cle.as_ptr(), valeur.as_ptr(), RRF_RT_REG_SZ, ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut cap) } != 0 {
        return None;
    }
    let s = depuis_utf16_nul(&buf);
    (!s.trim().is_empty()).then_some(s)
}

pub fn reg_dword(racine: isize, cle: &str, valeur: &str) -> Option<u32> {
    let cle = large(cle);
    let valeur = large(valeur);
    let mut v = 0u32;
    let mut cb = std::mem::size_of::<u32>() as u32;
    // SAFETY : écrit exactement un u32 dans `v`.
    (unsafe { RegGetValueW(racine, cle.as_ptr(), valeur.as_ptr(), RRF_RT_REG_DWORD, ptr::null_mut(), &mut v as *mut u32 as *mut c_void, &mut cb) } == 0)
        .then_some(v)
}

pub fn reg_qword(racine: isize, cle: &str, valeur: &str) -> Option<u64> {
    let cle = large(cle);
    let valeur = large(valeur);
    let mut v = 0u64;
    let mut cb = std::mem::size_of::<u64>() as u32;
    // SAFETY : écrit exactement un u64 dans `v` (QWORD ou binaire 8 octets pour la VRAM GPU).
    let ok = unsafe {
        RegGetValueW(
            racine,
            cle.as_ptr(),
            valeur.as_ptr(),
            RRF_RT_REG_QWORD | RRF_RT_REG_BINARY,
            ptr::null_mut(),
            &mut v as *mut u64 as *mut c_void,
            &mut cb,
        ) == 0
    };
    (ok && cb == 8).then_some(v)
}

pub fn reg_ecrire_chaine(racine: isize, cle: &str, valeur: &str, donnees: &str) -> Result<(), String> {
    let cle = large(cle);
    let valeur = large(valeur);
    let donnees = large(donnees);
    let mut h: isize = 0;
    // SAFETY : ouvre ou crée la sous-clé avec droit d'écriture ; refermée avant de rendre la main.
    let rc = unsafe { RegCreateKeyExW(racine, cle.as_ptr(), 0, ptr::null_mut(), 0, KEY_SET_VALUE, ptr::null(), &mut h, ptr::null_mut()) };
    if rc != 0 || h == 0 {
        return Err(format!("RegCreateKeyExW : code {rc}"));
    }
    // SAFETY : `donnees` est en UTF-16 terminé par NUL ; taille en octets NUL compris.
    let rc = unsafe { RegSetValueExW(h, valeur.as_ptr(), 0, REG_SZ, donnees.as_ptr() as *const u8, (donnees.len() * 2) as u32) };
    unsafe { RegCloseKey(h) };
    if rc == 0 { Ok(()) } else { Err(format!("RegSetValueExW : code {rc}")) }
}

pub fn reg_supprimer_valeur(racine: isize, cle: &str, valeur: &str) {
    let cle = large(cle);
    let valeur = large(valeur);
    let mut h: isize = 0;
    // SAFETY : ouvre la clé si elle existe, supprime la valeur, referme.
    if unsafe { RegOpenKeyExW(racine, cle.as_ptr(), 0, KEY_SET_VALUE, &mut h) } == 0 && h != 0 {
        unsafe {
            RegDeleteValueW(h, valeur.as_ptr());
            RegCloseKey(h);
        }
    }
}

fn reg_sous_cles(racine: isize, cle: &str) -> Vec<String> {
    let cle = large(cle);
    let mut h: isize = 0;
    // SAFETY : ouverture en lecture seule ; refermée en fin de boucle.
    if unsafe { RegOpenKeyExW(racine, cle.as_ptr(), 0, KEY_READ, &mut h) } != 0 || h == 0 {
        return vec![];
    }
    let mut out = vec![];
    let mut buf = [0u16; 260];
    for idx in 0..256u32 {
        let mut n = buf.len() as u32;
        // SAFETY : `buf` fait `n` caractères ; arrêt dès que l'énumération échoue.
        let rc = unsafe { RegEnumKeyExW(h, idx, buf.as_mut_ptr(), &mut n, ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) };
        if rc != 0 {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())]));
    }
    unsafe { RegCloseKey(h) };
    out
}

// ── Trousseau Windows (Gestionnaire d'identification / DPAPI) ────────────────

/// Lit un secret générique chiffré pour l'utilisateur courant dans le Gestionnaire d'identification.
pub fn cred_lire(cible: &str) -> Option<String> {
    let cible = large(cible);
    let mut p: *mut CredentialW = ptr::null_mut();
    // SAFETY : Windows alloue `CredentialW` si trouvé ; libéré par `CredFree` dans tous les cas.
    if unsafe { CredReadW(cible.as_ptr(), CRED_TYPE_GENERIC, 0, &mut p) } == 0 || p.is_null() {
        return None;
    }
    let secret = unsafe {
        let c = &*p;
        if c.credential_blob.is_null() || c.credential_blob_size == 0 || c.credential_blob_size > 65_536 {
            None
        } else {
            let b = std::slice::from_raw_parts(c.credential_blob, c.credential_blob_size as usize);
            // Écrit par L Katta : UTF-8. Modifié à la main dans le Panneau de configuration Windows :
            // UTF-16LE (un octet sur deux nul sur une clé ASCII). Les deux se lisent sans ambiguïté.
            let s = if b.len() >= 2 && b.len() % 2 == 0 && b[1] == 0 {
                let u: Vec<u16> = b.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect();
                depuis_utf16_nul(&u)
            } else {
                String::from_utf8_lossy(b).trim_end_matches('\0').to_string()
            };
            let s = s.trim().to_string();
            (!s.is_empty()).then_some(s)
        }
    };
    // SAFETY : tampon renvoyé par `CredReadW`, libéré une seule fois.
    unsafe { CredFree(p as *mut c_void) };
    secret
}

/// Enregistre un secret dans le Gestionnaire d'identification Windows (chiffré par DPAPI pour le
/// seul compte courant).
pub fn cred_ecrire(cible: &str, libelle: &str, secret: &str) -> Result<(), String> {
    let octets = secret.trim().as_bytes();
    if octets.is_empty() {
        return Err(t!("clé vide").into());
    }
    if octets.len() > CRED_MAX_BLOB {
        return Err(t!("secret trop long pour le trousseau Windows").into());
    }
    let mut cible = large(cible);
    let mut libelle = large(libelle);
    let mut user = large(&std::env::var("USERNAME").unwrap_or_else(|_| "lkatta".into()));
    let mut blob = octets.to_vec();
    let cred = CredentialW {
        flags: 0,
        typ: CRED_TYPE_GENERIC,
        target_name: cible.as_mut_ptr(),
        comment: libelle.as_mut_ptr(),
        last_written: [0, 0],
        credential_blob_size: blob.len() as u32,
        credential_blob: blob.as_mut_ptr(),
        persist: CRED_PERSIST_LOCAL_MACHINE,
        attribute_count: 0,
        attributes: ptr::null_mut(),
        target_alias: ptr::null_mut(),
        user_name: user.as_mut_ptr(),
    };
    // SAFETY : tous les tampons pointés vivent jusqu'au retour de `CredWriteW`.
    if unsafe { CredWriteW(&cred, 0) } != 0 {
        Ok(())
    } else {
        Err(format!("CredWriteW : {}", std::io::Error::last_os_error()))
    }
}

/// Retire un secret du Gestionnaire d'identification Windows.
pub fn cred_effacer(cible: &str) {
    let cible = large(cible);
    // SAFETY : chaîne UTF-16 terminée par NUL ; absent = sans effet.
    unsafe {
        CredDeleteW(cible.as_ptr(), CRED_TYPE_GENERIC, 0);
    }
}

// ── Langue, thème, occupation et inactivité ──────────────────────────────────

/// Langue préférée de l'utilisateur dans Windows (ex. « fr-FR », « fr-MG », « en-US »).
pub fn langue_systeme() -> Option<String> {
    let mut buf = [0u16; 85];
    // SAFETY : écrit au plus 85 caractères NUL compris.
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| depuis_utf16_nul(&buf))
}

/// Le thème d'applications de Windows est-il sombre ?
pub fn theme_sombre() -> bool {
    reg_dword(HKCU, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") == Some(0)
}

/// L'utilisateur est-il en mode présentation, plein écran exclusif, concentration ou écran
/// verrouillé ?
pub fn ne_pas_deranger() -> bool {
    let mut etat = 0i32;
    // SAFETY : écrit un entier `QUERY_USER_NOTIFICATION_STATE` ; S_OK == 0.
    if unsafe { SHQueryUserNotificationState(&mut etat) } != 0 {
        return false;
    }
    // 1 = écran verrouillé / absent, 2 = occupé / Ne pas déranger, 3 = jeu Direct3D plein écran,
    // 4 = mode présentation, 7 = application plein écran. 5 = libre, 6 = calme initial.
    matches!(etat, 1..=4 | 7)
}

/// Une application utilise-t-elle le microphone en ce moment (Windows 10 1903+ / Windows 11) ?
///
/// Windows tient à jour `LastUsedTimeStop = 0` dans `ConsentStore\microphone` tant qu'un flux
/// d'enregistrement est ouvert, sans ouvrir le périphérique audio ni déclencher d'indicateur.
pub fn micro_utilise() -> bool {
    const BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let actif = |cle: &str| {
        reg_qword(HKCU, cle, "LastUsedTimeStart").is_some_and(|s| s > 0)
            && reg_qword(HKCU, cle, "LastUsedTimeStop") == Some(0)
    };
    for k in reg_sous_cles(HKCU, BASE) {
        let sous = format!(r"{BASE}\{k}");
        if k.eq_ignore_ascii_case("NonPackaged") {
            if reg_sous_cles(HKCU, &sous).iter().any(|np| actif(&format!(r"{sous}\{np}"))) {
                return true;
            }
        } else if actif(&sous) {
            return true;
        }
    }
    false
}

/// Temps écoulé depuis la dernière touche ou le dernier mouvement de souris (en millisecondes).
pub fn inactif_ms() -> u64 {
    let mut info = LastInputInfo { cb_size: std::mem::size_of::<LastInputInfo>() as u32, dw_time: 0 };
    // SAFETY : `info` a la bonne taille ; soustraction modulo 2^32 (sûre après 49 jours).
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            0
        } else {
            GetTickCount().wrapping_sub(info.dw_time) as u64
        }
    }
}

// ── Matériel, système et santé ───────────────────────────────────────────────

/// Mémoire vive du processus courant (Working Set), en mégaoctets.
pub fn memoire_processus_mo() -> Option<u64> {
    let mut c = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY : pseudo-poignée du processus courant ; `c` a la taille annoncée.
    (unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0)
        .then_some((c.working_set_size / (1024 * 1024)) as u64)
}

/// Mémoire vive physique `(totale, disponible)` en octets.
pub fn memoire_vive() -> Option<(u64, u64)> {
    let mut m = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };
    // SAFETY : structure dimensionnée par `dw_length`.
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0 && m.ull_total_phys > 0)
        .then_some((m.ull_total_phys, m.ull_avail_phys))
}

/// Disques fixes montés : `(racine ex. "C:\\", octets_totaux, octets_disponibles)`.
pub fn disques() -> Vec<(String, u64, u64)> {
    // SAFETY : masque de bits des lettres A..Z, sans ressource.
    let masque = unsafe { GetLogicalDrives() };
    let mut out = vec![];
    for i in 0..26u32 {
        if masque & (1 << i) == 0 {
            continue;
        }
        let nom = format!("{}:\\", (b'A' + i as u8) as char);
        let w = large(&nom);
        // SAFETY : `w` est « X:\0 ».
        if unsafe { GetDriveTypeW(w.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let (mut dispo, mut total, mut libre) = (0u64, 0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut dispo, &mut total, &mut libre) } != 0 && total > 0 {
            out.push((nom, total, dispo));
        }
    }
    out
}

/// État de la batterie (`None` sur un ordinateur fixe sans batterie).
pub fn batterie() -> Option<String> {
    let mut s = SystemPowerStatus {
        ac_line_status: 255,
        battery_flag: 255,
        battery_life_percent: 255,
        system_status_flag: 0,
        battery_life_time: 0,
        battery_full_life_time: 0,
    };
    // SAFETY : écrit 12 octets dans `s`.
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 || s.battery_flag == 255 || (s.battery_flag & 128) != 0 || s.battery_life_percent > 100 {
        return None;
    }
    let etat = if (s.battery_flag & 8) != 0 {
        "en charge"
    } else if s.ac_line_status == 1 {
        "sur secteur"
    } else {
        "sur batterie"
    };
    Some(format!("{} % ({etat})", s.battery_life_percent))
}

/// Durée de fonctionnement depuis le démarrage, en secondes.
pub fn uptime_s() -> u64 {
    // SAFETY : compteur monotone du système.
    unsafe { GetTickCount64() / 1000 }
}

/// Nom exact de la version de Windows (distingue Windows 10 et Windows 11 par le numéro de build).
pub fn version_windows() -> (String, String) {
    let mut v = OsVersionInfoW {
        dw_os_version_info_size: std::mem::size_of::<OsVersionInfoW>() as u32,
        dw_major_version: 10,
        dw_minor_version: 0,
        dw_build_number: 0,
        dw_platform_id: 0,
        sz_csd_version: [0; 128],
    };
    // SAFETY : `RtlGetVersion` remplit la structure sans mentir sur Windows 11.
    let _ = unsafe { RtlGetVersion(&mut v) };
    let nom = if v.dw_major_version >= 10 && v.dw_build_number >= 22000 {
        "Windows 11"
    } else if v.dw_major_version == 10 {
        "Windows 10"
    } else {
        "Windows"
    };
    let rev = reg_chaine(HKLM, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "DisplayVersion").unwrap_or_default();
    let complet = if rev.is_empty() { nom.to_string() } else { format!("{nom} {rev}") };
    let build = format!("{}.{}.{}", v.dw_major_version, v.dw_minor_version, v.dw_build_number);
    (complet, build)
}

/// Modèle du processeur lu dans le registre matériel.
pub fn processeur() -> String {
    let nom = reg_chaine(HKLM, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| std::env::consts::ARCH.into());
    let n = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1);
    format!("{nom} ({n} cœurs logiques)")
}

/// Modèle de l'ordinateur (fabricant + nom), si renseigné par le firmware.
pub fn modele_machine() -> String {
    const CLE: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
    [reg_chaine(HKLM, CLE, "SystemManufacturer"), reg_chaine(HKLM, CLE, "SystemProductName")]
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("System Product Name") && !s.eq_ignore_ascii_case("To Be Filled By O.E.M."))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cartes graphiques installées et mémoire dédiée éventuelle (en Go).
pub fn cartes_graphiques() -> Vec<(String, Option<f32>)> {
    const CLASSE: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut out = vec![];
    for idx in 0..8 {
        let cle = format!(r"{CLASSE}\{idx:04}");
        let Some(nom) = reg_chaine(HKLM, &cle, "DriverDesc") else { continue };
        if nom.contains("Basic Display") || nom.contains("Remote Desktop") {
            continue;
        }
        let vram = reg_qword(HKLM, &cle, "HardwareInformation.qwMemorySize")
            .map(|b| (b as f64 / 1024f64.powi(3)) as f32)
            .filter(|g| *g >= 0.5);
        if !out.iter().any(|(n, _): &(String, Option<f32>)| n == &nom) {
            out.push((nom, vram));
        }
    }
    out
}

/// Application associée à un protocole (`https`, `mailto`) ou une extension (`.pdf`, `.png`…).
pub fn appli_par_defaut(cle_user_choice: &str) -> Option<String> {
    let prog_id = reg_chaine(HKCU, cle_user_choice, "ProgId")?;
    let p = prog_id.to_lowercase();
    let connu = if p.contains("firefox") {
        "Firefox"
    } else if p.contains("chrome") {
        "Google Chrome"
    } else if p.contains("msedge") {
        "Microsoft Edge"
    } else if p.contains("brave") {
        "Brave"
    } else if p.contains("vivaldi") {
        "Vivaldi"
    } else if p.contains("opera") {
        "Opera"
    } else if p.contains("zen") {
        "Zen Browser"
    } else if p.contains("thunderbird") {
        "Thunderbird"
    } else if p.contains("outlook") {
        "Outlook"
    } else if p.contains("sumatrapdf") {
        "SumatraPDF"
    } else if p.contains("acrobat") || p.contains("acrord") {
        "Adobe Acrobat"
    } else if p.contains("foxit") {
        "Foxit PDF"
    } else if p.contains("vscode") {
        "Visual Studio Code"
    } else if p.contains("notepad++") {
        "Notepad++"
    } else if p.contains("notepad") {
        "Bloc-notes"
    } else if p.contains("vlc") {
        "VLC"
    } else if p.contains("mpv") {
        "mpv"
    } else if p.contains("spotify") {
        "Spotify"
    } else if p.contains("irfanview") {
        "IrfanView"
    } else if p.contains("photos") {
        "Photos"
    } else {
        ""
    };
    if !connu.is_empty() {
        return Some(connu.into());
    }
    reg_chaine(HKLM, &format!(r"SOFTWARE\Classes\{prog_id}"), "FriendlyTypeName")
        .filter(|s| !s.starts_with('@'))
        .or_else(|| reg_chaine(HKCU, &format!(r"Software\Classes\{prog_id}"), ""))
        .filter(|s| !s.starts_with('@'))
}

// ── Moniteurs, DPI et capture d'écran ────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Moniteur {
    pub nom: String,
    pub ecran: Rect,
    pub travail: Rect,
    pub echelle: f32,
    pub principal: bool,
}

fn dpi_systeme() -> f32 {
    // SAFETY : DC de l'écran obtenu puis libéré aussitôt.
    unsafe {
        let dc = GetDC(0);
        if dc == 0 {
            return 1.0;
        }
        let ppp = GetDeviceCaps(dc, LOGPIXELSX);
        ReleaseDC(0, dc);
        if ppp <= 0 { 1.0 } else { (ppp as f32 / 96.0).clamp(1.0, 3.5) }
    }
}

pub fn echelle_moniteur(hmon: isize) -> f32 {
    static FN: OnceLock<Option<GetDpiForMonitorFn>> = OnceLock::new();
    let f = *FN.get_or_init(|| {
        let dll = large("shcore.dll");
        // SAFETY : charge shcore.dll (présente sur tout Windows 10/11) et résout GetDpiForMonitor.
        unsafe {
            let m = LoadLibraryW(dll.as_ptr());
            if m == 0 {
                return None;
            }
            let p = GetProcAddress(m, c"GetDpiForMonitor".as_ptr() as *const u8);
            if p.is_null() { None } else { Some(std::mem::transmute::<*const c_void, GetDpiForMonitorFn>(p)) }
        }
    });
    if let Some(get_dpi) = f {
        let (mut dx, mut dy) = (96u32, 96u32);
        // SAFETY : MDT_EFFECTIVE_DPI = 0 ; écrit deux u32.
        if hmon != 0 && unsafe { get_dpi(hmon, 0, &mut dx, &mut dy) } == 0 && dx > 0 {
            return (dx as f32 / 96.0).clamp(1.0, 3.5);
        }
    }
    dpi_systeme()
}

unsafe extern "system" fn collecter_moniteur(hmon: isize, _hdc: isize, _r: *const Rect, data: isize) -> i32 {
    let mut info = MonitorInfoExW {
        cb_size: std::mem::size_of::<MonitorInfoExW>() as u32,
        rc_monitor: Rect::default(),
        rc_work: Rect::default(),
        dw_flags: 0,
        sz_device: [0; 32],
    };
    // SAFETY : `info` est dimensionné ; `data` pointe vers notre `Vec<Moniteur>` sur la pile.
    if unsafe { GetMonitorInfoW(hmon, &mut info as *mut MonitorInfoExW as *mut c_void) } != 0 && info.rc_work.right > info.rc_work.left && info.rc_work.bottom > info.rc_work.top {
        let liste = unsafe { &mut *(data as *mut Vec<Moniteur>) };
        let nom = depuis_utf16_nul(&info.sz_device);
        let nom = nom.trim_start_matches(r"\\.\").to_string();
        liste.push(Moniteur {
            nom,
            ecran: info.rc_monitor,
            travail: info.rc_work,
            echelle: echelle_moniteur(hmon),
            principal: (info.dw_flags & MONITORINFOF_PRIMARY) != 0,
        });
    }
    1
}

/// Tous les moniteurs actifs, le moniteur principal en premier.
pub fn moniteurs() -> Vec<Moniteur> {
    let mut v: Vec<Moniteur> = Vec::new();
    // SAFETY : `v` vit sur la pile pendant toute la durée synchrone d'`EnumDisplayMonitors`.
    unsafe {
        EnumDisplayMonitors(0, ptr::null(), collecter_moniteur, &mut v as *mut Vec<Moniteur> as isize);
    }
    v.sort_by_key(|m| !m.principal);
    v
}

/// Nom du moniteur qui contient le point `(x, y)` (ou le plus proche).
pub fn moniteur_du_point(x: i32, y: i32) -> Option<String> {
    let mut info = MonitorInfoExW {
        cb_size: std::mem::size_of::<MonitorInfoExW>() as u32,
        rc_monitor: Rect::default(),
        rc_work: Rect::default(),
        dw_flags: 0,
        sz_device: [0; 32],
    };
    // SAFETY : renvoie le moniteur le plus proche et lit son nom.
    unsafe {
        let m = MonitorFromPoint(Point { x, y }, MONITOR_DEFAULTTONEAREST);
        if m == 0 || GetMonitorInfoW(m, &mut info as *mut MonitorInfoExW as *mut c_void) == 0 {
            return None;
        }
    }
    Some(depuis_utf16_nul(&info.sz_device).trim_start_matches(r"\\.\").to_string())
}

/// Nom du moniteur où se trouve le curseur de la souris.
pub fn moniteur_souris() -> Option<String> {
    let mut p = Point::default();
    // SAFETY : écrit les coordonnées du curseur.
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return None;
    }
    moniteur_du_point(p.x, p.y)
}

/// Capture le moniteur sous la souris en `Pixmap` RGBA (via GDI, sans outil externe).
pub fn capturer_ecran() -> Result<tiny_skia::Pixmap, String> {
    let mut pt = Point::default();
    let mut info = MonitorInfoExW {
        cb_size: std::mem::size_of::<MonitorInfoExW>() as u32,
        rc_monitor: Rect::default(),
        rc_work: Rect::default(),
        dw_flags: 0,
        sz_device: [0; 32],
    };
    // SAFETY : choisit le moniteur du curseur (ou le principal par repli).
    let rect = unsafe {
        let _ = GetCursorPos(&mut pt);
        let m = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if m != 0 && GetMonitorInfoW(m, &mut info as *mut MonitorInfoExW as *mut c_void) != 0 {
            info.rc_monitor
        } else {
            return Err(t!("aucun écran disponible pour la capture").into());
        }
    };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0 || h <= 0 || w > 16_384 || h > 16_384 {
        return Err(t!("taille d'écran invalide").into());
    }
    // SAFETY : DC écran, DC mémoire et DIB 32 bits top-down libérés sur tous les chemins.
    unsafe {
        let ecran = GetDC(0);
        if ecran == 0 {
            return Err(t!("capture d'écran refusée par le système").into());
        }
        let dc = CreateCompatibleDC(ecran);
        if dc == 0 {
            ReleaseDC(0, ecran);
            return Err(t!("capture d'écran impossible").into());
        }
        let hdr = BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: w,
            bi_height: -h,
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        let mut bits: *mut c_void = ptr::null_mut();
        let bmp = CreateDIBSection(dc, &hdr, 0, &mut bits, 0, 0);
        if bmp == 0 || bits.is_null() {
            DeleteDC(dc);
            ReleaseDC(0, ecran);
            return Err(t!("mémoire insuffisante pour la capture").into());
        }
        let ancien = SelectObject(dc, bmp);
        let ok = BitBlt(dc, 0, 0, w, h, ecran, rect.left, rect.top, SRCCOPY) != 0;
        let resultat = if ok {
            let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32).ok_or_else(|| t!("image invalide").to_string());
            if let Ok(ref mut pm) = pm {
                let src = std::slice::from_raw_parts(bits as *const u8, (w as usize) * (h as usize) * 4);
                for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    // GDI écrit BGRX : rouge et bleu échangés, alpha forcé à 255 (opaque).
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = 255;
                }
            }
            pm
        } else {
            Err(t!("la copie de l'écran a échoué").into())
        };
        SelectObject(dc, ancien);
        DeleteDC(dc);
        DeleteObject(bmp);
        ReleaseDC(0, ecran);
        resultat
    }
}

/// Transforme une image RGBA prémultipliée (`tiny-skia`) en icône Win32 (`HICON`) à canal alpha.
/// L'appelant libère la poignée avec `DestroyIcon` à la fermeture.
pub fn icone_depuis_pixmap(pm: &tiny_skia::Pixmap) -> isize {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    if w <= 0 || h <= 0 {
        return 0;
    }
    // SAFETY : crée un DIB 32 bits BGRA et un masque monochrome temporaires ; `CreateIconIndirect`
    // copie leurs pixels, donc les deux bitmaps sont détruits immédiatement après.
    unsafe {
        let ecran = GetDC(0);
        if ecran == 0 {
            return 0;
        }
        let hdr = BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: w,
            bi_height: -h,
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        let mut bits: *mut c_void = ptr::null_mut();
        let couleur = CreateDIBSection(ecran, &hdr, 0, &mut bits, 0, 0);
        ReleaseDC(0, ecran);
        if couleur == 0 || bits.is_null() {
            return 0;
        }
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w as usize) * (h as usize) * 4);
        for (d, s) in dst.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
            // Les icônes 32 bits Win32 attendent du BGRA non prémultiplié.
            let a = s[3];
            if a == 0 {
                d.copy_from_slice(&[0, 0, 0, 0]);
            } else {
                let demul = |c: u8| (((c as u32) * 255 + (a as u32 / 2)) / (a as u32)).min(255) as u8;
                d[0] = demul(s[2]);
                d[1] = demul(s[1]);
                d[2] = demul(s[0]);
                d[3] = a;
            }
        }
        let masque = CreateBitmap(w, h, 1, 1, ptr::null());
        if masque == 0 {
            DeleteObject(couleur);
            return 0;
        }
        let info = IconInfo { f_icon: 1, x_hotspot: 0, y_hotspot: 0, hbm_mask: masque, hbm_color: couleur };
        let icone = CreateIconIndirect(&info);
        DeleteObject(masque);
        DeleteObject(couleur);
        icone
    }
}

// ── Notifications du bureau (via l'icône de la zone de notification) ─────────

static FILE_NOTIFS: OnceLock<Mutex<Vec<(String, String)>>> = OnceLock::new();

pub fn pousser_notification(titre: &str, texte: &str) -> Result<(), String> {
    let mut f = FILE_NOTIFS.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| "verrou".to_string())?;
    if f.len() < 16 {
        f.push((titre.to_string(), texte.to_string()));
    }
    Ok(())
}

pub fn prendre_notifications() -> Vec<(String, String)> {
    let Some(m) = FILE_NOTIFS.get() else { return vec![] };
    m.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
}

// ── Raccourcis du menu Démarrer (.lnk) ───────────────────────────────────────

/// Dossiers du menu Démarrer Windows (utilisateur d'abord, puis machine).
pub fn dossiers_menu_demarrer() -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Some(a) = std::env::var_os("APPDATA") {
        v.push(std::path::PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(p) = std::env::var_os("ProgramData") {
        v.push(std::path::PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    v
}

/// Lance un raccourci `.lnk` appartenant strictement au menu Démarrer (jamais un chemin arbitraire).
pub fn lancer_raccourci_menu(chemin: &Path) -> Result<(), String> {
    let ext_ok = chemin.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
    let dans_menu = dossiers_menu_demarrer().iter().any(|d| chemin.starts_with(d));
    if !ext_ok || !dans_menu || !chemin.is_file() {
        return Err(t!("raccourci hors du menu Démarrer refusé").into());
    }
    let op = large("open");
    let cible = large_os(chemin.as_os_str());
    // SAFETY : chaînes UTF-16 terminées par NUL ; limité aux `.lnk` existants du menu Démarrer.
    let r = unsafe { ShellExecuteW(0, op.as_ptr(), cible.as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL) };
    if r < 0 || (r as usize) < 32 {
        Err(format!("ShellExecuteW a refusé (code {r})"))
    } else {
        Ok(())
    }
}

// ── Empreinte SHA-256 native (CNG / bcrypt.dll) ──────────────────────────────

/// Calcule l'empreinte SHA-256 hexadécimale d'un fichier via Windows CNG (`bcrypt.dll`).
pub fn sha256_fichier(chemin: &Path) -> Result<String, String> {
    let octets = std::fs::read(chemin).map_err(|e| e.to_string())?;
    sha256_octets(&octets)
}

pub fn sha256_octets(octets: &[u8]) -> Result<String, String> {
    let len = u32::try_from(octets.len()).map_err(|_| "fichier trop grand".to_string())?;
    let nom = large("SHA256");
    let mut alg: isize = 0;
    // SAFETY : ouvre le fournisseur SHA-256 intégré à Windows, hache, referme.
    let rc = unsafe { BCryptOpenAlgorithmProvider(&mut alg, nom.as_ptr(), ptr::null(), 0) };
    if rc != 0 || alg == 0 {
        return Err(format!("BCryptOpenAlgorithmProvider : {rc:#x}"));
    }
    let mut out = [0u8; 32];
    let ptr_in = if octets.is_empty() { ptr::null() } else { octets.as_ptr() };
    let rc = unsafe { BCryptHash(alg, ptr::null(), 0, ptr_in, len, out.as_mut_ptr(), out.len() as u32) };
    unsafe { BCryptCloseAlgorithmProvider(alg, 0) };
    if rc != 0 {
        return Err(format!("BCryptHash : {rc:#x}"));
    }
    let mut hex = String::with_capacity(64);
    for b in out {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    Ok(hex)
}

// ── Format ICO (PNG embarqué) ────────────────────────────────────────────────

/// Construit un fichier `.ico` valide contenant une icône PNG (256×256 ou autre).
pub fn ico_depuis_png(png: &[u8], taille: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(22 + png.len());
    // ICONDIR : réservé (0), type (1 = icône), nombre d'images (1).
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    // ICONDIRENTRY : largeur (0 = 256), hauteur (0 = 256), couleurs (0), réservé (0), plans (1),
    // bits (32), taille des données, décalage (22).
    v.push(taille);
    v.push(taille);
    v.push(0);
    v.push(0);
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(&(png.len() as u32).to_le_bytes());
    v.extend_from_slice(&22u32.to_le_bytes());
    v.extend_from_slice(png);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_primitives_behave_honestly() {
        let (tot, dispo) = memoire_vive().expect("mémoire vive lisible");
        assert!(tot > 256 * 1024 * 1024 && dispo <= tot);
        let mo = memoire_processus_mo().expect("WorkingSet lisible");
        assert!(mo > 0 && mo < 2048, "WorkingSet suspect : {mo} Mo");
        assert!(!disques().is_empty(), "au moins un disque fixe");
        let (os, build) = version_windows();
        assert!(os.starts_with("Windows") && build.contains('.'), "{os} / {build}");
        assert!(!processeur().is_empty());
        assert_eq!(
            sha256_octets(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_octets(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        fil_discret();
    }

    #[test]
    fn credential_manager_roundtrip() {
        let cible = format!("lkatta:test-ci-{}", std::process::id());
        cred_effacer(&cible);
        assert_eq!(cred_lire(&cible), None);
        cred_ecrire(&cible, "Essai L Katta", "secret-de-test-42").unwrap();
        assert_eq!(cred_lire(&cible).as_deref(), Some("secret-de-test-42"));
        cred_effacer(&cible);
        assert_eq!(cred_lire(&cible), None);
    }
}
