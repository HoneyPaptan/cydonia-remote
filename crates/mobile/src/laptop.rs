use crate::net::Endpoint;
use futures::channel::mpsc::{self, UnboundedSender};
use gui::model::relay::{self, Opened, Shells};
use remote::proto::{Query, Screen, ShellInput};
use std::{
    cell::RefCell,
    rc::Rc,
};
use wasm_bindgen::{JsCast as _, closure::Closure};
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

thread_local! {
    static ENDPOINT: RefCell<Option<Rc<Endpoint>>> = const { RefCell::new(None) };
}

fn endpoint() -> Option<Rc<Endpoint>> {
    ENDPOINT.with(|held| held.borrow().clone())
}

fn fetch(query: Query) {
    let Some(endpoint) = endpoint() else {
        relay::unanswered(&query);
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        match endpoint.query(&query).await {
            Ok(answer) => relay::answered(query, answer),
            Err(_) => relay::unanswered(&query),
        }
    });
}

pub fn install(endpoint: Rc<Endpoint>) {
    let host = web_sys::window()
        .and_then(|window| window.location().hostname().ok())
        .unwrap_or_default();
    ENDPOINT.with(|held| *held.borrow_mut() = Some(endpoint));
    relay::install(fetch, &host);
    relay::install_shells(Rc::new(Sockets));
}

struct Sockets;

struct Socket {
    socket: WebSocket,
    waiting: Rc<RefCell<Option<Vec<String>>>>,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _close: Closure<dyn FnMut(CloseEvent)>,
    _open: Closure<dyn FnMut(Event)>,
}

impl Socket {
    fn send(&self, input: &ShellInput) {
        let Ok(text) = serde_json::to_string(input) else {
            return;
        };
        if let Some(waiting) = self.waiting.borrow_mut().as_mut() {
            waiting.push(text);
            return;
        }
        let _ = self.socket.send_with_str(&text);
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        self.socket.set_onopen(None);
        let _ = self.socket.close();
    }
}

fn flush(socket: &WebSocket, waiting: &RefCell<Option<Vec<String>>>) {
    for text in waiting.borrow_mut().take().unwrap_or_default() {
        let _ = socket.send_with_str(&text);
    }
}

impl Shells for Sockets {
    fn open(&self, cwd: &str, cols: u16, rows: u16) -> Option<Opened> {
        let endpoint = endpoint()?;
        let socket = endpoint.shell(cwd, cols, rows).ok()?;
        let (screens, receiver) = mpsc::unbounded::<Screen>();
        let sender: Rc<RefCell<Option<UnboundedSender<Screen>>>> =
            Rc::new(RefCell::new(Some(screens)));
        let arriving = sender.clone();
        let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            if let (Ok(screen), Some(sender)) =
                (serde_json::from_str::<Screen>(&text), arriving.borrow().as_ref())
            {
                let _ = sender.unbounded_send(screen);
            }
        });
        let close = Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
            sender.borrow_mut().take();
        });
        let waiting = Rc::new(RefCell::new(Some(Vec::new())));
        let open = {
            let socket = socket.clone();
            let waiting = waiting.clone();
            Closure::<dyn FnMut(Event)>::new(move |_: Event| flush(&socket, &waiting))
        };
        socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
        socket.set_onclose(Some(close.as_ref().unchecked_ref()));
        socket.set_onopen(Some(open.as_ref().unchecked_ref()));
        let held = Rc::new(Socket {
            socket,
            waiting,
            _message: message,
            _close: close,
            _open: open,
        });
        Some(Opened {
            input: Rc::new(move |input| held.send(&input)),
            screens: receiver,
        })
    }
}
