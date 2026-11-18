use super::*;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use zbus::{
    Connection,
    message::Header,
    zvariant::{Type, Value},
};
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";

#[derive(Default)]
struct Shared {
    path: Mutex<String>,
    sender: Mutex<String>,
    result: AtomicU32,
    bindings: AtomicU32,
    registered: Mutex<HashSet<String>>,
    require_identity: AtomicBool,
    reject_registration: AtomicBool,
    sessions: AtomicU32,
}
struct Mock(Arc<Shared>);
struct MockRegistry(Arc<Shared>);
#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl MockRegistry {
    #[zbus(property)]
    fn version(&self) -> u32 {
        1
    }
    async fn register(
        &self,
        app_id: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<()> {
        assert_eq!(app_id, crate::APPLICATION_ID);
        if self.0.reject_registration.load(Ordering::SeqCst) {
            return Err(zbus::fdo::Error::AccessDenied("Registration denied".into()));
        }
        assert!(
            self.0
                .registered
                .lock()
                .unwrap()
                .insert(header.sender().unwrap().as_str().into()),
            "registered the same connection twice"
        );
        Ok(())
    }
}
#[derive(Serialize, Type)]
#[zvariant(signature = "dict")]
struct Created {
    #[serde(with = "zbus::zvariant::as_value")]
    session_handle: String,
}
#[derive(Serialize, Type)]
#[zvariant(signature = "dict")]
struct Bound {
    #[serde(with = "zbus::zvariant::as_value")]
    shortcuts: Vec<(String, HashMap<String, Value<'static>>)>,
}
fn path(
    header: &Header<'_>,
    options: &HashMap<String, OwnedValue>,
    kind: &str,
    token: &str,
) -> String {
    let sender = header
        .sender()
        .unwrap()
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let token = options.get(token).unwrap().downcast_ref::<&str>().unwrap();
    format!("{OBJECT}/{kind}/{sender}/{token}")
}
struct MockSession;
#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl MockSession {
    async fn close(&self) {}
}
struct MockRequest;
#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl MockRequest {
    async fn close(&self) {}
}
#[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
impl Mock {
    #[zbus(property)]
    fn version(&self) -> u32 {
        2
    }
    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        if self.0.require_identity.load(Ordering::SeqCst)
            && !self
                .0
                .registered
                .lock()
                .unwrap()
                .contains(header.sender().unwrap().as_str())
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "An app id is required".into(),
            ));
        }
        self.0.sessions.fetch_add(1, Ordering::SeqCst);
        let request = path(&header, &options, "request", "handle_token");
        let session = path(&header, &options, "session", "session_handle_token");
        *self.0.path.lock().unwrap() = session.clone();
        connection
            .object_server()
            .at(session.as_str(), MockSession)
            .await
            .unwrap();
        connection
            .object_server()
            .at(request.as_str(), MockRequest)
            .await
            .unwrap();
        connection
            .emit_signal(
                None::<&str>,
                request.as_str(),
                REQUEST,
                "Response",
                &(
                    0u32,
                    Created {
                        session_handle: session,
                    },
                ),
            )
            .await
            .unwrap();
        Ok(request.try_into().unwrap())
    }
    async fn bind_shortcuts(
        &self,
        session: OwnedObjectPath,
        shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
        _parent: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert_eq!(session.as_str(), self.0.path.lock().unwrap().as_str());
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].0, "ptt");
        *self.0.sender.lock().unwrap() = header.sender().unwrap().as_str().into();
        let request = path(&header, &options, "request", "handle_token");
        connection
            .object_server()
            .at(request.as_str(), MockRequest)
            .await
            .unwrap();
        self.0.bindings.fetch_add(1, Ordering::SeqCst);
        let result = self.0.result.load(Ordering::SeqCst);
        if result != 99 {
            connection
                .emit_signal(
                    None::<&str>,
                    request.as_str(),
                    REQUEST,
                    "Response",
                    &(
                        result,
                        Bound {
                            shortcuts: vec![(
                                "ptt".into(),
                                HashMap::from([
                                    ("description".into(), Value::from("Synthetic PTT")),
                                    ("trigger_description".into(), Value::from("Ctrl+F8")),
                                ]),
                            )],
                        },
                    ),
                )
                .await
                .unwrap();
        }
        Ok(request.try_into().unwrap())
    }
}
async fn state(worker: &mut crate::Worker, revision: u64, wanted: State, level: Level) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let sample = worker.view.borrow_and_update().clone();
            if sample.revision == revision && sample.state == wanted && sample.level == level {
                break;
            }
            worker.view.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}
async fn signal(connection: &Connection, path: &str, id: &str, member: &str) {
    // Equal millisecond timestamps deliberately exercise wire-order preservation.
    connection
        .emit_signal(
            None::<&str>,
            OBJECT,
            SHORTCUTS,
            member,
            &(
                OwnedObjectPath::try_from(path).unwrap(),
                id,
                1u64,
                HashMap::<String, Value<'_>>::new(),
            ),
        )
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires the validation script's private dbus-run-session"]
async fn private_portal_order_denial_cancellation_and_disconnect() {
    assert_eq!(std::env::var("SRS_PRIVATE_INPUT_BUS").as_deref(), Ok("1"));
    assert!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap()
            .contains("/tmp/dbus-")
    );
    let shared = Arc::new(Shared::default());
    shared.require_identity.store(true, Ordering::SeqCst);
    let service = zbus::connection::Builder::session()
        .unwrap()
        .name(PORTAL)
        .unwrap()
        .serve_at(OBJECT, Mock(shared.clone()))
        .unwrap()
        .serve_at(OBJECT, MockRegistry(shared.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    let mut worker = crate::Worker::spawn().unwrap();
    let binding = crate::Mode::Keyboard {
        description: "Private test shortcut".into(),
        key: 8,
        modifiers: 2,
    };
    let revision = worker.configure(binding.clone()).unwrap();
    state(&mut worker, revision, State::Ready, Level::Unknown).await;
    assert_eq!(worker.view.borrow().trigger, "Ctrl+F8");
    let path = shared.path.lock().unwrap().clone();
    signal(&service, &path, "ptt", "Activated").await;
    signal(&service, &path, "ptt", "Deactivated").await;
    state(&mut worker, revision, State::Ready, Level::Released).await;
    signal(&service, "/unrelated", "ptt", "Activated").await;
    signal(&service, &path, "other", "Activated").await;
    tokio::time::sleep(POLL * 3).await;
    assert_eq!(worker.view.borrow().level, Level::Released);
    signal(&service, &path, "ptt", "Activated").await;
    state(&mut worker, revision, State::Ready, Level::Held).await;
    signal(&service, &path, "ptt", "Deactivated").await;
    state(&mut worker, revision, State::Ready, Level::Released).await;
    signal(&service, &path, "ptt", "Activated").await;
    state(&mut worker, revision, State::Ready, Level::Held).await;
    service
        .emit_signal(
            None::<&str>,
            path.as_str(),
            "org.freedesktop.portal.Session",
            "Closed",
            &(HashMap::<String, Value<'_>>::new(),),
        )
        .await
        .unwrap();
    state(&mut worker, revision, State::Disconnected, Level::Unknown).await;

    // A registration denial must not fall through to CreateSession/BindShortcuts.
    let sessions = shared.sessions.load(Ordering::SeqCst);
    shared.reject_registration.store(true, Ordering::SeqCst);
    let revision = worker.configure(binding.clone()).unwrap();
    state(&mut worker, revision, State::Unavailable, Level::Unknown).await;
    assert_eq!(shared.sessions.load(Ordering::SeqCst), sessions);
    shared.reject_registration.store(false, Ordering::SeqCst);

    shared.result.store(1, Ordering::SeqCst);
    let revision = worker.configure(binding.clone()).unwrap();
    state(&mut worker, revision, State::Denied, Level::Unknown).await;
    shared.result.store(99, Ordering::SeqCst); // Leave the chooser response pending.
    let calls = shared.bindings.load(Ordering::SeqCst);
    worker.configure(binding.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while shared.bindings.load(Ordering::SeqCst) == calls {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .unwrap();
    let sender = shared.sender.lock().unwrap().clone();
    let revision = worker.configure(crate::Mode::Off).unwrap();
    state(&mut worker, revision, State::Off, Level::Unknown).await;
    let dbus = zbus::fdo::DBusProxy::new(&service).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while dbus
            .name_has_owner(sender.as_str().try_into().unwrap())
            .await
            .unwrap()
        {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .expect("canceled portal setup retained its private connection");
    shared.result.store(0, Ordering::SeqCst);
    // Old portals lack Registry; only that missing interface permits fallback.
    service
        .object_server()
        .remove::<MockRegistry, _>(OBJECT)
        .await
        .unwrap();
    shared.require_identity.store(false, Ordering::SeqCst);
    let registered = shared.registered.lock().unwrap().len();
    let revision = worker.configure(binding.clone()).unwrap();
    state(&mut worker, revision, State::Ready, Level::Unknown).await;
    assert_eq!(shared.registered.lock().unwrap().len(), registered);
    let revision = worker.configure(crate::Mode::Off).unwrap();
    state(&mut worker, revision, State::Off, Level::Unknown).await;
    service
        .object_server()
        .at(OBJECT, MockRegistry(shared.clone()))
        .await
        .unwrap();
    shared.require_identity.store(true, Ordering::SeqCst);
    let revision = worker.configure(binding).unwrap();
    state(&mut worker, revision, State::Ready, Level::Unknown).await;
    assert_eq!(shared.registered.lock().unwrap().len(), registered + 1);
    let path = shared.path.lock().unwrap().clone();
    signal(&service, &path, "ptt", "Deactivated").await;
    state(&mut worker, revision, State::Ready, Level::Released).await;
    signal(&service, &path, "ptt", "Activated").await;
    state(&mut worker, revision, State::Ready, Level::Held).await;
    service.release_name(PORTAL).await.unwrap();
    state(&mut worker, revision, State::Disconnected, Level::Unknown).await;
}
