//! Test the real rfd portal transport against our private chooser service.
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use zbus::{
    Connection,
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};
struct Choice {
    save: bool,
    code: u32,
    uri: String,
}
struct Chooser(Arc<Mutex<VecDeque<Choice>>>);
impl Chooser {
    async fn reply(
        &self,
        save: bool,
        title: String,
        options: HashMap<String, OwnedValue>,
        header: Header<'_>,
        connection: &Connection,
    ) -> OwnedObjectPath {
        assert_eq!(title, "Private preset chooser");
        assert!(options.contains_key("filters"));
        let choice = self.0.lock().unwrap().pop_front().unwrap();
        assert_eq!(choice.save, save);
        let sender = header
            .sender()
            .unwrap()
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let token = options["handle_token"].downcast_ref::<&str>().unwrap();
        let path: OwnedObjectPath =
            format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
                .try_into()
                .unwrap();
        let signal_path = path.clone();
        let connection = connection.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let uris = if choice.code == 0 {
                vec![choice.uri]
            } else {
                vec![]
            };
            connection
                .emit_signal(
                    None::<&str>,
                    &signal_path,
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(choice.code, HashMap::from([("uris", Value::from(uris))])),
                )
                .await
                .unwrap();
        });
        path
    }
}
#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }
    async fn open_file(
        &self,
        _parent: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> OwnedObjectPath {
        self.reply(false, title, options, header, connection).await
    }
    async fn save_file(
        &self,
        _parent: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> OwnedObjectPath {
        self.reply(true, title, options, header, connection).await
    }
}
async fn result(worker: &Worker) -> Result<Data, StoreError> {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(result) = worker.poll() {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "requires test_native_dialogs.py private D-Bus"]
async fn private_native_chooser_cancel_export_import_and_conflicts() {
    assert_eq!(std::env::var("SRS_PRIVATE_CHOOSER").as_deref(), Ok("1"));
    assert!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap()
            .contains("/tmp/dbus-")
    );
    let choices = Arc::new(Mutex::new(VecDeque::new()));
    let _service = zbus::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Chooser(choices.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profiles.json");
    let bundle = directory.path().join("radio bundle.json");
    let worker = Worker::start(Some(profile.clone())).unwrap();
    let empty = result(&worker).await.unwrap();
    let mut data = Data::default();
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251e6, Modulation::AM);
    data.save_preset(Preset::new("Fixture", 1, &radios, &DEFAULT_MONITOR).unwrap())
        .unwrap();
    let command = |save: bool, data: Data| {
        if save {
            Command::Export {
                data,
                title: "Private preset chooser".into(),
                filter: "Radio presets".into(),
            }
        } else {
            Command::Import {
                data,
                title: "Private preset chooser".into(),
                filter: "Radio presets".into(),
            }
        }
    };
    let choose = |save, code| {
        choices.lock().unwrap().push_back(Choice {
            save,
            code,
            uri: format!("file://{}", bundle.to_str().unwrap().replace(' ', "%20")),
        })
    };
    choose(false, 1);
    worker.send(command(false, empty.clone())).unwrap();
    assert_eq!(result(&worker).await.unwrap_err(), StoreError::Canceled);
    assert!(!profile.exists());
    choose(true, 0);
    worker.send(command(true, data.clone())).unwrap();
    result(&worker).await.unwrap();
    let bytes = std::fs::read(&bundle).unwrap();
    choose(true, 0);
    worker.send(command(true, data.clone())).unwrap();
    assert_eq!(result(&worker).await.unwrap_err(), StoreError::Exists);
    assert_eq!(std::fs::read(&bundle).unwrap(), bytes);
    choose(false, 0);
    worker.send(command(false, empty)).unwrap();
    let imported = result(&worker).await.unwrap();
    assert_eq!(imported.presets, data.presets);
    let saved = std::fs::read(&profile).unwrap();
    choose(false, 0);
    worker.send(command(false, imported.clone())).unwrap();
    assert_eq!(result(&worker).await.unwrap_err(), StoreError::NameConflict);
    std::fs::write(&bundle, b"invalid JSON").unwrap();
    choose(false, 0);
    worker.send(command(false, imported)).unwrap();
    assert_eq!(result(&worker).await.unwrap_err(), StoreError::Invalid);
    assert_eq!(std::fs::read(&profile).unwrap(), saved);
    assert!(choices.lock().unwrap().is_empty());
}
