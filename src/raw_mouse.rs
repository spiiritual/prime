//! Raw mouse movement for the Aim Trainer, read the way VALORANT reads it: Raw Input counts,
//! before Windows pointer speed and acceleration. iced 0.14 drops winit's raw device events, so a
//! thread with its own message-only window reads them. Windows sends a process's raw mouse input
//! to one window, so this takes it from winit while a run plays; iced ignores it anyway.
//!
//! When the Windows cursor turns a 3D view, each packet instead reads how far the cursor moved
//! since the last one, so its movement counts with Windows' pointer speed and acceleration. It's
//! put back at the arena's centre only near the edge: putting it back after every packet lost the
//! movement that arrived in between, about a tenth of it, and more or less with speed.
//!
//! Counts and clicks are gathered here and taken once per frame: a message per packet rebuilt
//! the whole UI up to a thousand times a second, which starved the repaints. Each click keeps the
//! counts read before it, so it shoots where the crosshair was when the button went down.

use std::ptr::{null, null_mut};
use std::sync::Mutex;

use iced::futures::{SinkExt, Stream};
use iced::{Point, Rectangle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ClipCursor, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClipCursor,
    GetCursorPos, GetMessageW, GetWindowRect, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SetCursorPos, WM_CLOSE, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;
/// `RI_MOUSE_LEFT_BUTTON_DOWN`.
const LEFT_BUTTON_DOWN: u16 = 0x0001;

/// How the cursor is held while a run plays.
static GRAB: Mutex<Option<Grab>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Grab {
    /// Where the cursor is kept. winit clears any clip it didn't set itself (when iced hides the
    /// cursor, and when the cursor enters or leaves the window), so the reader puts it back on
    /// each movement.
    clip: RECT,
    /// Where the cursor is put back near the edge while the Windows cursor turns a 3D view.
    centre: Option<POINT>,
    /// Where the cursor was after the last packet, which its next move counts from.
    last: POINT,
}

static INPUT: Mutex<Input> = Mutex::new(Input {
    clicks: Vec::new(),
    moved: (0, 0),
});

/// Mouse input read since the last frame took it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// Each left click, oldest first, with the counts read between it and the click or take
    /// before it.
    pub clicks: Vec<(i32, i32)>,
    /// The counts read since the last click or take.
    pub moved: (i32, i32),
}

impl Input {
    /// One packet; its movement comes before its button.
    fn read(&mut self, (dx, dy): (i32, i32), left_down: bool) {
        self.moved = (
            self.moved.0.saturating_add(dx),
            self.moved.1.saturating_add(dy),
        );
        if left_down {
            self.clicks.push(std::mem::take(&mut self.moved));
        }
    }
}

/// The reader couldn't start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

/// Reads raw mouse input for `take_input` and keeps the cursor held while subscribed. Yields
/// only if the reader couldn't start. Ending the subscription closes the reader's window, which
/// gives the raw mouse input back.
pub fn reader() -> impl Stream<Item = Unavailable> {
    iced::stream::channel(1, async |mut output| {
        let Some(_reader) = Reader::start() else {
            let _ = output.send(Unavailable).await;
            return;
        };
        std::future::pending::<()>().await;
    })
}

/// The input read since the last take.
pub fn take_input() -> Input {
    std::mem::take(&mut *INPUT.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Holds the cursor on its current pixel, inside Prime, so a fast flick can't click elsewhere.
pub fn hold_cursor() {
    // Tests must never clip the real cursor.
    if cfg!(test) {
        return;
    }
    let mut point = POINT::default();
    // SAFETY: only writes the local value passed to it.
    if unsafe { GetCursorPos(&mut point) } != 0 {
        grab(Grab {
            clip: RECT {
                left: point.x,
                top: point.y,
                right: point.x + 1,
                bottom: point.y + 1,
            },
            centre: None,
            last: point,
        });
    }
}

/// Keeps the cursor inside `area` of Prime's window, in iced's logical pixels, so it can't click
/// beside the arena. Called from `update`, on the window's thread, while Prime is active.
pub fn confine_cursor(area: Rectangle) {
    // Tests must never clip the real cursor.
    if cfg!(test) {
        return;
    }
    if let Some((clip, _)) = on_screen(area) {
        grab(Grab {
            clip,
            centre: None,
            last: POINT::default(),
        });
    }
}

/// Confines the cursor to `area`, starting at its centre, and reads how far it moves as counts. Returns the window's physical pixels per logical one, or `None` if the
/// window couldn't be found. Called like `confine_cursor`.
pub fn recentre_cursor(area: Rectangle) -> Option<f64> {
    // Tests must never move the real cursor.
    if cfg!(test) {
        return None;
    }
    let (clip, scale) = on_screen(area)?;
    let centre = POINT {
        x: (clip.left + clip.right) / 2,
        y: (clip.top + clip.bottom) / 2,
    };
    grab(Grab {
        clip,
        centre: Some(centre),
        last: centre,
    });
    Some(scale.into())
}

/// `area` of the active window, in iced's logical pixels, on screen and rounded inwards so the
/// cursor never sits on its border, with the window's scale.
fn on_screen(area: Rectangle) -> Option<(RECT, f32)> {
    // SAFETY: the calls only read the window and write the local values passed to them.
    unsafe {
        let window = GetActiveWindow();
        if window.is_null() {
            return None;
        }
        let dpi = GetDpiForWindow(window);
        let mut origin = POINT::default();
        if dpi == 0 || ClientToScreen(window, &mut origin) == 0 {
            return None;
        }
        let scale = dpi as f32 / 96.0;
        Some((
            RECT {
                left: origin.x + (area.x * scale).ceil() as i32,
                top: origin.y + (area.y * scale).ceil() as i32,
                right: origin.x + ((area.x + area.width) * scale).floor() as i32,
                bottom: origin.y + ((area.y + area.height) * scale).floor() as i32,
            },
            scale,
        ))
    }
}

/// The active window's top-left corner and its monitor's bounds, in the window's logical pixels.
pub fn window_and_monitor() -> Option<(Point, Rectangle)> {
    // Tests have no window to move.
    if cfg!(test) {
        return None;
    }
    // SAFETY: the calls only read the window and monitor and write the local values passed to them.
    unsafe {
        let window = GetActiveWindow();
        if window.is_null() {
            return None;
        }
        let dpi = GetDpiForWindow(window);
        let mut frame = RECT::default();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        if dpi == 0
            || GetWindowRect(window, &mut frame) == 0
            || GetMonitorInfoW(monitor, &mut info) == 0
        {
            return None;
        }
        let scale = dpi as f32 / 96.0;
        let bounds = info.rcMonitor;
        Some((
            Point::new(frame.left as f32 / scale, frame.top as f32 / scale),
            Rectangle {
                x: bounds.left as f32 / scale,
                y: bounds.top as f32 / scale,
                width: (bounds.right - bounds.left) as f32 / scale,
                height: (bounds.bottom - bounds.top) as f32 / scale,
            },
        ))
    }
}

fn grab(grab: Grab) {
    // The lock stays held so a packet can't read the cursor before it's centred.
    let mut held = GRAB.lock().unwrap_or_else(|e| e.into_inner());
    *held = Some(grab);
    // SAFETY: only reads the local rectangle, or moves the cursor.
    unsafe {
        ClipCursor(&grab.clip);
        if let Some(centre) = grab.centre {
            SetCursorPos(centre.x, centre.y);
        }
    }
}

/// Keeps the grab up for one packet and returns the counts it stands for: the raw ones, or how
/// far the cursor moved when it turns a 3D view. The clip is only put back on a change, since
/// each clip sends the window a WM_MOUSEMOVE.
fn keep_grab(counts: (i32, i32)) -> (i32, i32) {
    // The lock stays held so a release can't slip in between.
    let mut guard = GRAB.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Grab { clip, centre, last }) = &mut *guard else {
        return counts;
    };
    let (clip, centre) = (*clip, *centre);
    // SAFETY: the calls only read or write the local values passed to them, or move the cursor.
    unsafe {
        let mut active = RECT::default();
        if counts != (0, 0)
            && GetClipCursor(&mut active) != 0
            && (active.left, active.top, active.right, active.bottom)
                != (clip.left, clip.top, clip.right, clip.bottom)
        {
            ClipCursor(&clip);
        }
        let Some(centre) = centre else {
            return counts;
        };
        let mut at = POINT::default();
        if GetCursorPos(&mut at) == 0 {
            return (0, 0);
        }
        let (moved, back) = follow(at, *last, centre, clip);
        *last = if back { centre } else { at };
        if back {
            SetCursorPos(centre.x, centre.y);
        }
        moved
    }
}

/// How far the cursor moved from `last` to `at`, and whether to put it back at `centre`, which
/// it is once it's a quarter of the clip's width or height away. Moves land between reading the cursor and
/// putting it back are lost, so it goes back as rarely as the clip allows; a quarter leaves room
/// for the fastest flick between two packets.
fn follow(at: POINT, last: POINT, centre: POINT, clip: RECT) -> ((i32, i32), bool) {
    let moved = (at.x - last.x, at.y - last.y);
    let near_edge = (at.x - centre.x).abs() > (clip.right - clip.left) / 4
        || (at.y - centre.y).abs() > (clip.bottom - clip.top) / 4;
    (moved, near_edge)
}

pub fn release_cursor() {
    // Tests must never touch the real cursor.
    if cfg!(test) {
        return;
    }
    let mut held = GRAB.lock().unwrap_or_else(|e| e.into_inner());
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
    fn start() -> Option<Self> {
        // Input left from before would jump the crosshair.
        take_input();
        let (ready, started) = std::sync::mpsc::channel();
        std::thread::spawn(move || read_on_this_thread(ready));
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

fn read_on_this_thread(ready: std::sync::mpsc::Sender<Option<usize>>) {
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
            if let Some((counts, left_down)) = unsafe { read_packet(lparam) } {
                // Every packet recentres, so a click's counts include the cursor's last move.
                let moved = keep_grab(counts);
                INPUT
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .read(moved, left_down);
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

/// Relative movement from one WM_INPUT, and whether the left button went down. Absolute devices
/// (tablets, remote desktop) give no counts, but their packets still recentre the cursor.
///
/// # Safety
///
/// `lparam` must be the `lParam` of a WM_INPUT message being handled, so it is a valid raw input
/// handle.
unsafe fn read_packet(lparam: LPARAM) -> Option<((i32, i32), bool)> {
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
    // SAFETY: both halves of the union are plain integers.
    let left_down = unsafe { mouse.Anonymous.Anonymous.usButtonFlags } & LEFT_BUTTON_DOWN != 0;
    let counts = if mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
        (mouse.lLastX, mouse.lLastY)
    } else {
        (0, 0)
    };
    Some((counts, left_down))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_turns_from_where_it_was_and_goes_back_only_near_the_edge() {
        let clip = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 200,
        };
        let centre = POINT { x: 200, y: 100 };
        // Inside the middle half it stays where it went, and the next move counts from there.
        assert_eq!(
            follow(POINT { x: 230, y: 90 }, centre, centre, clip),
            ((30, -10), false)
        );
        assert_eq!(
            follow(
                POINT { x: 290, y: 90 },
                POINT { x: 230, y: 90 },
                centre,
                clip
            ),
            ((60, 0), false)
        );
        // Past a quarter of the width or height it goes back to the centre.
        assert_eq!(
            follow(
                POINT { x: 310, y: 90 },
                POINT { x: 290, y: 90 },
                centre,
                clip
            ),
            ((20, 0), true)
        );
        assert_eq!(
            follow(POINT { x: 200, y: 151 }, centre, centre, clip),
            ((0, 51), true)
        );
    }

    #[test]
    fn each_click_keeps_the_counts_read_before_it() {
        let mut input = Input::default();
        input.read((10, 0), false);
        input.read((5, 1), true);
        input.read((20, 0), false);
        input.read((0, 0), true);
        input.read((3, 3), false);
        assert_eq!(
            input,
            Input {
                clicks: vec![(15, 1), (20, 0)],
                moved: (3, 3),
            }
        );
    }
}
