use super::*;
#[tokio::test]
#[ignore = "run explicitly with the private native-input validation script"]
async fn virtual_joystick_button_and_disconnect() {
    let mut slot = None;
    let context = context(&mut slot).unwrap();
    let connection = context
        .subsystem
        .attach_virtual_joystick(
            sdl3::joystick::VirtualJoystickDescription::new()
                .name("SRS synthetic PTT")
                .with_buttons([sdl3::gamepad::Button::South, sdl3::gamepad::Button::East]),
        )
        .unwrap();
    let id = connection.id().raw();
    let control = context.open(id).unwrap();
    let (sender, mut view) = tokio::sync::watch::channel(crate::Snapshot::default());
    let mut publisher = Publisher::new(sender, 1);
    assert_eq!(
        run(&mut slot, id, 127, &mut publisher).await,
        Err(State::Failed)
    );
    async fn wait(view: &mut tokio::sync::watch::Receiver<crate::Snapshot>, level: Level) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if view.borrow_and_update().level == level {
                    break;
                }
                view.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    let drive = async move {
        wait(&mut view, Level::Released).await;
        control.set_virtual_button(1, true).unwrap();
        tokio::time::sleep(POLL * 3).await;
        assert_eq!(
            view.borrow().level,
            Level::Released,
            "unbound button triggered PTT"
        );
        control.set_virtual_button(0, true).unwrap();
        wait(&mut view, Level::Held).await;
        control.set_virtual_button(0, false).unwrap();
        wait(&mut view, Level::Released).await;
        control.set_virtual_button(0, true).unwrap();
        wait(&mut view, Level::Held).await;
        drop(connection);
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(run(&mut slot, id, 0, &mut publisher), drive)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(State::Disconnected));
}
