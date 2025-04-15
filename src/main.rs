use anyhow::{Result, anyhow};
use std::ffi::CString;
use std::ptr;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::Security::*;
use windows::core::{PCSTR, PSTR};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use std::sync::atomic::{AtomicU32, Ordering, AtomicBool};
use chrono::Local;
use std::path::Path;
use std::fs::create_dir_all;
use image;
use image::GenericImageView;
use std::sync::mpsc;

// Global variable to track Chrome process ID for window enumeration
static CHROME_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

static SCREENSHOT_SAVED: AtomicBool = AtomicBool::new(false);

// Create a hidden desktop and switch the current thread to it
unsafe fn create_hidden_desktop(desktop_name: &str) -> Result<HDESK> {
    let desktop_name = CString::new(desktop_name)?;
    let mut sd = SECURITY_DESCRIPTOR::default();
    if InitializeSecurityDescriptor(PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _), 1).is_err() {
        return Err(anyhow!("Failed to initialize security descriptor"));
    }
    if SetSecurityDescriptorDacl(PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _), true, None, false).is_err() {
        return Err(anyhow!("Failed to set security descriptor DACL"));
    }
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: &mut sd as *mut _ as *mut _,
        bInheritHandle: true.into(),
    };
    let access_mask = DESKTOP_CREATEWINDOW.0 |
        DESKTOP_WRITEOBJECTS.0 |
        DESKTOP_SWITCHDESKTOP.0 |
        DESKTOP_READOBJECTS.0 |
        DESKTOP_ENUMERATE.0 |
        0x10000000; // GENERIC_ALL
    let desktop = CreateDesktopA(
        PCSTR(desktop_name.as_ptr() as *const u8),
        PCSTR::null(),
        None,
        DESKTOP_CONTROL_FLAGS(0),
        access_mask,
        Some(&mut sa),
    )?;
    if desktop.is_invalid() {
        return Err(anyhow!("Failed to create hidden desktop"));
    }
    if SetThreadDesktop(desktop).is_err() {
        return Err(anyhow!("Failed to set thread desktop"));
    }
    Ok(desktop)
}

// Launch Chrome in the specified desktop
unsafe fn launch_chrome_on_desktop(desktop_name: &str, chrome_path: &str) -> Result<PROCESS_INFORMATION> {
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let mut si = STARTUPINFOA::default();
    si.cb = std::mem::size_of::<STARTUPINFOA>() as u32;
    let desktop_path = format!("WinSta0\\{}", desktop_name);
    let desktop_cstring = CString::new(desktop_path)?;
    si.lpDesktop = PSTR(desktop_cstring.as_ptr() as *mut u8);
    let mut command = format!(
        "\"{}\" --disable-gpu --disable-software-rasterizer --new-window https://example.com",
        chrome_path
    );
    let mut pi = PROCESS_INFORMATION::default();
    let result = CreateProcessA(
        PCSTR::null(),
        PSTR(command.as_mut_ptr()),
        Some(&mut sa),
        Some(&mut sa),
        true,
        PROCESS_CREATION_FLAGS(CREATE_NEW_CONSOLE.0 | NORMAL_PRIORITY_CLASS.0 | CREATE_DEFAULT_ERROR_MODE.0),
        None,
        PCSTR::null(),
        &si,
        &mut pi,
    );
    if !result.is_ok() {
        return Err(anyhow!("Failed to launch Chrome: {:?}", GetLastError()));
    }
    ResumeThread(pi.hThread);
    Ok(pi)
}

// Find the first Chrome process ID
fn find_chrome_process_id() -> Option<u32> {
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return None,
        };
        if snapshot.is_invalid() {
            return None;
        }
        let mut process_entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut result = Process32FirstW(snapshot, &mut process_entry);
        while result.is_ok() {
            let proc_name = String::from_utf16_lossy(
                &process_entry.szExeFile[..process_entry.szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(process_entry.szExeFile.len())]
            ).to_lowercase();
            if proc_name.contains("chrome") {
                let process_id = process_entry.th32ProcessID;
                CloseHandle(snapshot);
                return Some(process_id);
            }
            result = Process32NextW(snapshot, &mut process_entry);
        }
        CloseHandle(snapshot);
        None
    }
}

// Helper to get window title
fn get_window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}

// Screenshot logic: enumerate windows, find Chrome, and capture
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    let mut process_id: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    let target_process_id = CHROME_PROCESS_ID.load(Ordering::Relaxed);
    if process_id == target_process_id {
        let visible = IsWindowVisible(hwnd).as_bool();
        let title = get_window_title(hwnd);
        println!("[HVNC thread] Found window: HWND={:?}, Visible={}, Title='{}'", hwnd, visible, title);
        if visible && !title.trim().is_empty() {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                if rect.right > rect.left && rect.bottom > rect.top {
                    println!("[+] Found visible Chrome window with title '{}', attempting screenshot...", title);
                    match capture_window_to_file(hwnd, None) {
                        Ok(path) => {
                            println!("[+] Screenshot saved to: {}", path);
                            SCREENSHOT_SAVED.store(true, Ordering::Relaxed);
                        },
                        Err(e) => {
                            println!("[-] Screenshot failed: {:?}", e);
                        }
                    }
                } else {
                    println!("[-] Chrome window has no size, skipping screenshot");
                }
            } else {
                println!("[-] Failed to get Chrome window dimensions");
            }
        } else {
            println!("[-] Skipping window: not visible or empty title");
        }
    }
    BOOL(1)
}

// Capture a window to a PNG file
fn capture_window_to_file(hwnd: HWND, specific_path: Option<&str>) -> Result<String, windows::core::Error> {
    unsafe {
        let window_dc = GetWindowDC(hwnd);
        if window_dc.is_invalid() {
            return Err(windows::core::Error::from_win32());
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            ReleaseDC(hwnd, window_dc);
            return Err(windows::core::Error::from_win32());
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let memory_dc = CreateCompatibleDC(window_dc);
        if memory_dc.is_invalid() {
            ReleaseDC(hwnd, window_dc);
            return Err(windows::core::Error::from_win32());
        }
        let bitmap = CreateCompatibleBitmap(window_dc, width, height);
        if bitmap.is_invalid() {
            DeleteDC(memory_dc);
            ReleaseDC(hwnd, window_dc);
            return Err(windows::core::Error::from_win32());
        }
        let old_bitmap = SelectObject(memory_dc, bitmap);
        if BitBlt(memory_dc, 0, 0, width, height, window_dc, 0, 0, SRCCOPY).is_err() {
            SelectObject(memory_dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(hwnd, window_dc);
            return Err(windows::core::Error::from_win32());
        }
        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default(); 1],
        };
        let bytes_per_pixel = 4;
        let stride = width * bytes_per_pixel;
        let buffer_size = (stride * height) as usize;
        let mut buffer = vec![0u8; buffer_size];
        let result = GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bitmap_info,
            DIB_RGB_COLORS
        );
        if result == 0 {
            SelectObject(memory_dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(hwnd, window_dc);
            return Err(windows::core::Error::from_win32());
        }
        let mut img_buffer = image::RgbaImage::new(width as u32, height as u32);
        for y in 0..height as u32 {
            for x in 0..width as u32 {
                let pixel_pos = ((y * width as u32 + x) * 4) as usize;
                let b = buffer[pixel_pos];
                let g = buffer[pixel_pos + 1];
                let r = buffer[pixel_pos + 2];
                let a = buffer[pixel_pos + 3];
                img_buffer.put_pixel(x, y, image::Rgba([r, g, b, a]));
            }
        }
        let file_path = match specific_path {
            Some(path) => path.to_string(),
            None => {
                let screenshots_dir = Path::new("screenshots");
                if !screenshots_dir.exists() {
                    create_dir_all(screenshots_dir).map_err(|_| windows::core::Error::from_win32())?;
                }
                let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
                format!("screenshots/chrome_screenshot_{}.png", timestamp)
            }
        };
        img_buffer.save(&file_path).map_err(|_| windows::core::Error::from_win32())?;
        SelectObject(memory_dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        ReleaseDC(hwnd, window_dc);
        Ok(file_path)
    }
}

// Find all Chrome process IDs
fn find_all_chrome_process_ids() -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return pids,
        };
        if snapshot.is_invalid() {
            return pids;
        }
        let mut process_entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut result = Process32FirstW(snapshot, &mut process_entry);
        while result.is_ok() {
            let proc_name = String::from_utf16_lossy(
                &process_entry.szExeFile[..process_entry.szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(process_entry.szExeFile.len())]
            ).to_lowercase();
            if proc_name.contains("chrome") {
                pids.push(process_entry.th32ProcessID);
            }
            result = Process32NextW(snapshot, &mut process_entry);
        }
        let _ = CloseHandle(snapshot);
    }
    pids
}

fn is_image_blank<P: AsRef<std::path::Path>>(path: P) -> bool {
    match image::open(&path) {
        Ok(img) => {
            let mut unique = None;
            for pixel in img.pixels() {
                let rgba = pixel.2.0;
                if let Some(u) = unique {
                    if u != rgba {
                        return false; // Found a different pixel
                    }
                } else {
                    unique = Some(rgba);
                }
            }
            true // All pixels are the same
        },
        Err(_) => true, // If can't open, treat as blank
    }
}

fn main() -> Result<()> {
    let screenshots_dir = std::path::Path::new("screenshots");
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(screenshots_dir)?;
    }
    let desktop_name = "ChromeHVNC";
    let chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe"; // Change if needed
    println!("[+] Creating hidden desktop...");
    let hidden_desktop = unsafe { create_hidden_desktop(desktop_name)? };
    println!("[+] Hidden desktop created.");
    println!("[+] Launching Chrome in hidden desktop...");
    let _chrome_proc = unsafe { launch_chrome_on_desktop(desktop_name, chrome_path)? };
    println!("[+] Chrome launched. Waiting for it to initialize...");
    std::thread::sleep(std::time::Duration::from_secs(10)); // Wait longer for Chrome to fully start
    println!("[+] Searching for Chrome processes...");
    let chrome_pids = find_all_chrome_process_ids();
    if chrome_pids.is_empty() {
        return Err(anyhow!("Could not find any Chrome process"));
    }
    println!("[+] Found Chrome PIDs: {:?}", chrome_pids);

    // Channel to get result from the capture thread
    let (tx, rx) = mpsc::channel();
    let desktop_handle = hidden_desktop;
    let chrome_pids_clone = chrome_pids.clone();
    let screenshots_dir = screenshots_dir.to_path_buf();
    std::thread::spawn(move || {
        // Switch this thread to the hidden desktop
        unsafe {
            if SetThreadDesktop(desktop_handle).is_err() {
                let _ = tx.send(Err(anyhow!("Failed to set thread desktop in capture thread")));
                return;
            }
            // Run a short message loop to allow Chrome to paint, but do not block
            let mut msg = MSG::default();
            let start = std::time::Instant::now();
            while start.elapsed().as_secs() < 3 {
                while PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE).as_bool() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let mut any_valid_screenshot = false;
        for pid in chrome_pids_clone {
            println!("[+] [HVNC thread] Attempting to capture Chrome window for PID {}...", pid);
            CHROME_PROCESS_ID.store(pid, Ordering::Relaxed);
            unsafe {
                let enum_result = EnumWindows(Some(enum_windows_proc), LPARAM(0));
                if enum_result.is_err() {
                    println!("[-] [HVNC thread] EnumWindows failed for PID {}", pid);
                    continue;
                }
            }
            if SCREENSHOT_SAVED.load(Ordering::Relaxed) {
                // Find the most recent screenshot file
                let mut latest: Option<std::path::PathBuf> = None;
                if let Ok(entries) = std::fs::read_dir(&screenshots_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map(|e| e == "png").unwrap_or(false) {
                            if let Some(ref l) = latest {
                                if let (Ok(meta1), Ok(meta2)) = (std::fs::metadata(&path), std::fs::metadata(l)) {
                                    if meta1.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH) > meta2.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH) {
                                        latest = Some(path);
                                    }
                                }
                            } else {
                                latest = Some(path);
                            }
                        }
                    }
                }
                if let Some(ref img_path) = latest {
                    if is_image_blank(img_path) {
                        println!("[-] [HVNC thread] Screenshot at {:?} is blank (all pixels the same).", img_path);
                    } else {
                        println!("[+] [HVNC thread] Screenshot at {:?} is a valid Chrome window!", img_path);
                        any_valid_screenshot = true;
                    }
                }
                // Reset for next PID in case of multiple
                SCREENSHOT_SAVED.store(false, Ordering::Relaxed);
            }
        }
        if any_valid_screenshot {
            let _ = tx.send(Ok(()));
        } else {
            let _ = tx.send(Err(anyhow!("No valid Chrome window screenshot was saved. Make sure Chrome is visible in the hidden desktop and try again.")));
        }
    });
    // Wait for the result from the capture thread
    match rx.recv() {
        Ok(Ok(())) => {
            println!("[+] Done. Check the screenshots folder for valid Chrome window images.");
            Ok(())
        },
        Ok(Err(e)) => Err(e),
        Err(e) => Err(anyhow!("Failed to receive from capture thread: {}", e)),
    }
}
