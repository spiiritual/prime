//! Raw mouse movement for the Aim Trainer, read the way VALORANT reads it: Raw Input counts,
//! before Windows pointer speed and acceleration. iced 0.14 drops winit's raw device events, so a
//! thread with its own message-only window reads them. Windows sends a process's raw mouse input
//! to one window, so this takes it from winit while a run plays; iced ignores it anyway.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use iced::futures::{SinkExt, Stream};
use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ClipCursor, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClipCursor,
    GetCursorPos, GetMessageW, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage, RegisterClassW, WM_CLOSE,
    WM_DESTROY, WM_INPUT, WNDCLASSW,
};

const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;

/// The pixel the cursor is held on while a run plays. winit clears any clip it didn't set itself
/// (when iced hides the cursor, and when the cursor enters or leaves the window), so the reader
/// puts it back on each movement.
static HELD: Mutex<Option<RECT>> = Mutex::new(None);

thread_local! {
    static SENDER: RefCell<Option<UnboundedSender<(i32, i32)>>> = const { RefCell::new(None) };
}

/// What the reader reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawMouse {
    Moved(i32, i32),
    /// The reader couldn't start; sent once, then the stream ends.
    Unavailable,
}

/// Relative mouse counts while subscribed. Ending the subscription closes the reader's window,
/// which gives the raw mouse input back.
pub fn movements() -> impl Stream<Item = RawMouse> {
    iced::stream::channel(64, async |mut output| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let Some(_reader) = Reader::start(sender) else {
            let _ = output.send(RawMouse::Unavailable).await;
            return;
        };
        // A 1000 Hz mouse sends a message every millisecond; one per wake-up is plenty.
        while let Some((mut dx, mut dy)) = receiver.recv().await {
            while let Ok((x, y)) = receiver.try_recv() {
                dx += x;
                dy += y;
            }
            if output.send(RawMouse::Moved(dx, dy)).await.is_err() {
                break;
            }
        }
    })
}

/// Holds the cursor on its current pixel, inside Prime, so a fast flick can't click elsewhere.
pub fn hold_cursor() {
    // Tests must never clip the real cursor.
    if cfg!(test) {
        return;
    }
    let mut point = POINT::default();
    // SAFETY: both calls only read or write the local values passed to them.
    unsafe {
        if GetCursorPos(&mut point) != 0 {
            let rect = RECT {
                left: point.x,
                top: point.y,
                right: point.x + 1,
                bottom: point.y + 1,
            };
            let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
            *held = Some(rect);
            ClipCursor(&rect);
        }
    }
}

/// Puts the hold back if something cleared it. Only clips on a change, since each clip sends
/// the window a WM_MOUSEMOVE.
fn keep_cursor_held() {
    // The lock stays held while clipping so a release can't slip in between.
    let guard = HELD.lock().unwrap_or_else(|e| e.into_inner());
    let Some(held) = *guard else {
        return;
    };
    let mut active = RECT::default();
    // SAFETY: both calls only read or write the local values passed to them.
    unsafe {
        if GetClipCursor(&mut active) != 0
            && (active.left, active.top, active.right, active.bottom)
                != (held.left, held.top, held.right, held.bottom)
        {
            ClipCursor(&held);
        }
    }
}

pub fn release_cursor() {
    // Tests must never touch the real cursor.
    if cfg!(test) {
        return;
    }
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    *held = None;
    // SAFETY: a null rectangle frees the cursor.
    unsafe {
        ClipCursor(null());
    }
    drop(held);
}

/// The reader thread's window, kept as an address because `HWND` isn't `Send`.
struct Reader {
    window: usize,
}

impl Reader {
    fn start(sender: UnboundedSender<(i32, i32)>) -> Option<Self> {
        let (ready, started) = std::sync::mpsc::channel();
        std::thread::spawn(move || read_on_this_thread(sender, ready));
        started.recv().ok().flatten().map(|window| Self { window })
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: posting to a window that's already gone just fails.
        unsafe {
            PostMessageW(self.window as HWND, WM_CLOSE, 0, 0);
        }
    }
}

fn read_on_this_thread(
    sender: UnboundedSender<(i32, i32)>,
    ready: std::sync::mpsc::Sender<Option<usize>>,
) {
    SENDER.with_borrow_mut(|slot| *slot = Some(sender));
    let class_name: Vec<u16> = "PrimeRawMouse".encode_utf16().chain([0]).collect();

    // SAFETY: `class_name` outlives the window; the window and its messages stay on this thread.
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        // Fails harmlessly once the class exists from an earlier run.
        RegisterClassW(&class);
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            let _ = ready.send(None);
            return;
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: GENERIC_DESKTOP,
            usUsage: MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        if RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            // Destroying the window runs WM_DESTROY's RIDEV_REMOVE, which only drops winit's
            // registration this call just replaced, so nothing else loses mouse input.
            DestroyWindow(window);
            let _ = ready.send(None);
            return;
        }
        let _ = ready.send(Some(window as usize));

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_INPUT => {
            // SAFETY: Windows passes the raw input handle in `lparam` for WM_INPUT.
            if let Some(movement) = unsafe { read_movement(lparam) } {
                keep_cursor_held();
                SENDER.with_borrow(|sender| {
                    if let Some(sender) = sender {
                        let _ = sender.send(movement);
                    }
                });
            }
            // SAFETY: WM_INPUT must reach DefWindowProc so Windows can clean up.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_DESTROY => {
            let device = RAWINPUTDEVICE {
                usUsagePage: GENERIC_DESKTOP,
                usUsage: MOUSE,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: null_mut(),
            };
            // SAFETY: `device` is a local value; quitting ends this thread's message loop.
            unsafe {
                RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32);
                PostQuitMessage(0);
            }
            0
        }
        // SAFETY: everything else gets Windows' default handling, including WM_CLOSE.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Relative movement from one WM_INPUT; absolute devices (tablets, remote desktop) are ignored.
///
/// # Safety
///
/// `lparam` must be the `lParam` of a WM_INPUT message being handled, so it is a valid raw input
/// handle.
unsafe fn read_movement(lparam: LPARAM) -> Option<(i32, i32)> {
    let mut input = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    // SAFETY: `input` is large enough for a mouse packet and `size` says so.
    let read = unsafe {
        GetRawInputData(
            lparam as HRAWINPUT,
            RID_INPUT,
            (&raw mut input).cast(),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || input.header.dwType != RIM_TYPEMOUSE {
        return None;
    }
    // SAFETY: the header says this packet is a mouse's.
    let mouse = unsafe { input.data.mouse };
    let relative = mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0;
    (relative && (mouse.lLastX != 0 || mouse.lLastY != 0)).then_some((mouse.lLastX, mouse.lLastY))
}
