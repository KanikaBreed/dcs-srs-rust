use super::*;
use x11rb_async::protocol::{
    xproto::{InputFocus, KEY_PRESS_EVENT, KEY_RELEASE_EVENT},
    xtest::ConnectionExt as _,
};
#[tokio::test]
#[ignore = "requires the validation script's private authenticated Xvfb display"]
async fn private_x11_background_key_and_modifiers() {
    assert!(
        std::env::var("XAUTHORITY")
            .unwrap_or_default()
            .contains("xvfb-run.")
    );
    let (connection, screen, drive) = RustConnection::connect(None).await.unwrap();
    let trial = async {
        let root = connection.setup().roots[screen].root;
        connection
            .set_input_focus(InputFocus::POINTER_ROOT, root, 0u32)
            .await
            .unwrap()
            .check()
            .await
            .unwrap();
        let setup = connection.setup();
        let mapping = connection
            .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
            .await
            .unwrap()
            .reply()
            .await
            .unwrap();
        let code = |symbol| {
            setup.min_keycode
                + mapping
                    .keysyms
                    .chunks(usize::from(mapping.keysyms_per_keycode))
                    .position(|keys| keys.contains(&symbol))
                    .unwrap() as u8
        };
        let f8 = code(0xffc5);
        let ctrl = code(0xffe3);
        let mut worker = crate::Worker::spawn().unwrap();
        worker
            .configure(crate::Mode::Keyboard {
                description: "Private test shortcut".into(),
                key: 8,
                modifiers: 0,
            })
            .unwrap();
        async fn wait(worker: &mut crate::Worker, level: Level) {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let sample = worker.view.borrow_and_update().clone();
                    if sample.state == State::Ready && sample.level == level {
                        break;
                    }
                    worker.view.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
        }
        wait(&mut worker, Level::Released).await;
        for (key, pressed, level) in [
            (f8, true, Level::Held),
            (ctrl, true, Level::Unknown),
            (f8, false, Level::Released),
            (ctrl, false, Level::Released),
            (f8, true, Level::Held),
            (f8, false, Level::Released),
        ] {
            connection
                .xtest_fake_input(
                    if pressed {
                        KEY_PRESS_EVENT
                    } else {
                        KEY_RELEASE_EVENT
                    },
                    key,
                    0,
                    root,
                    0,
                    0,
                    0,
                )
                .await
                .unwrap()
                .check()
                .await
                .unwrap();
            wait(&mut worker, level).await;
        }
        worker.configure(crate::Mode::Off).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while worker.view.borrow_and_update().state != State::Off {
                worker.view.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    };
    tokio::select! { result = drive => panic!("private X11 driver stopped: {result:?}"), () = trial => () }
}
