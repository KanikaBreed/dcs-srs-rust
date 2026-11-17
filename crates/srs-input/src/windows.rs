use crate::{Level, POLL, State, worker::Publisher};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
fn active_desktop() -> bool {
    use windows_sys::Win32::System::{
        StationsAndDesktops::{GetThreadDesktop, GetUserObjectInformationW, UOI_IO},
        Threading::GetCurrentThreadId,
    };
    let mut active = 0i32;
    // SAFETY: GetThreadDesktop borrows this live worker's desktop handle; it must
    // not be closed. UOI_IO writes one BOOL into aligned stack storage of exactly
    // that size. Neither pointer escapes the synchronous call. No desktop/input
    // state is changed. A failed query is unknown, never a synthetic release.
    unsafe {
        let desktop = GetThreadDesktop(GetCurrentThreadId());
        !desktop.is_null()
            && GetUserObjectInformationW(
                desktop,
                UOI_IO,
                (&mut active as *mut i32).cast(),
                std::mem::size_of::<i32>() as u32,
                std::ptr::null_mut(),
            ) != 0
            && active != 0
    }
}
fn down(key: i32) -> bool {
    // SAFETY: GetAsyncKeyState takes a scalar virtual-key code and returns a
    // scalar. It borrows no memory, transfers no ownership, and is thread-safe.
    // Read only its current-down bit; the historical low bit is unreliable.
    unsafe { GetAsyncKeyState(key) < 0 }
}
pub(crate) async fn run(key: u8, modifiers: u8, publisher: &mut Publisher) -> Result<(), State> {
    loop {
        let held = down(0x70 + i32::from(key) - 1);
        let actual = u8::from(down(0x10))
            | (u8::from(down(0x11)) << 1)
            | (u8::from(down(0x12)) << 2)
            | (u8::from(down(0x5b) || down(0x5c)) << 3);
        publisher.sample(if !active_desktop() {
            Level::Unknown
        } else if !held {
            Level::Released
        } else if actual == modifiers {
            Level::Held
        } else {
            Level::Unknown
        });
        tokio::time::sleep(POLL).await;
    }
}
