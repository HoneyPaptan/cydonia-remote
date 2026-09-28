use futures::channel::mpsc::UnboundedSender;
use remote::proto::{Ack, Command, Frame, Snapshot};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CloseEvent, Headers, MessageEvent, Request, RequestInit, Response, WebSocket};

const TOKEN_KEY: &str = "token=";
const PROTOCOL: &str = "cydonia";

pub struct Endpoint {
    pub base: String,
    pub token: String,
}

pub enum Inbound {
    Frame(Frame),
    Closed,
}

pub struct Socket {
    socket: WebSocket,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _close: Closure<dyn FnMut(CloseEvent)>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

fn window() -> Result<web_sys::Window, String> {
    web_sys::window().ok_or_else(|| "no window".to_owned())
}

fn text(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

impl Endpoint {
    pub fn from_location() -> Result<Self, String> {
        let location = window()?.location();
        let base = location.origin().map_err(text)?;
        let hash = location.hash().map_err(text)?;
        let token = hash
            .trim_start_matches('#')
            .split('&')
            .find_map(|part| part.strip_prefix(TOKEN_KEY))
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "no token in the address".to_owned())?;
        Ok(Self {
            base,
            token: token.to_owned(),
        })
    }

    async fn call(&self, method: &str, path: &str, body: Option<String>) -> Result<String, String> {
        let headers = Headers::new().map_err(text)?;
        headers
            .set("Authorization", &format!("Bearer {}", self.token))
            .map_err(text)?;
        let init = RequestInit::new();
        init.set_method(method);
        if let Some(body) = body {
            headers
                .set("Content-Type", "application/json")
                .map_err(text)?;
            init.set_body(&JsValue::from_str(&body));
        }
        init.set_headers(&headers);
        let request =
            Request::new_with_str_and_init(&format!("{}{path}", self.base), &init).map_err(text)?;
        let response: Response = JsFuture::from(window()?.fetch_with_request(&request))
            .await
            .map_err(text)?
            .dyn_into()
            .map_err(text)?;
        if !response.ok() {
            return Err(format!("{path} answered {}", response.status()));
        }
        JsFuture::from(response.text().map_err(text)?)
            .await
            .map_err(text)?
            .as_string()
            .ok_or_else(|| "a body that is not text".to_owned())
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let body = self.call("GET", "/v1/snapshot", None).await?;
        serde_json::from_str(&body).map_err(|error| error.to_string())
    }

    pub async fn command(&self, command: &Command) -> Result<Ack, String> {
        let body = serde_json::to_string(command).map_err(|error| error.to_string())?;
        let answer = self.call("POST", "/v1/commands", Some(body)).await?;
        serde_json::from_str(&answer).map_err(|error| error.to_string())
    }

    pub fn socket_url(&self, epoch: u64, seq: u64) -> String {
        let base = self
            .base
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        format!("{base}/v1/events?epoch={epoch}&seq={seq}")
    }

    pub fn subscribe(
        &self,
        epoch: u64,
        seq: u64,
        inbound: UnboundedSender<Inbound>,
    ) -> Result<Socket, String> {
        let protocols = js_sys::Array::of2(&PROTOCOL.into(), &self.token.as_str().into());
        let socket = WebSocket::new_with_str_sequence(&self.socket_url(epoch, seq), &protocols)
            .map_err(text)?;
        let frames = inbound.clone();
        let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(body) = event.data().as_string() else {
                return;
            };
            if let Ok(frame) = serde_json::from_str::<Frame>(&body) {
                let _ = frames.unbounded_send(Inbound::Frame(frame));
            }
        });
        let close = Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
            let _ = inbound.unbounded_send(Inbound::Closed);
        });
        socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
        socket.set_onclose(Some(close.as_ref().unchecked_ref()));
        Ok(Socket {
            socket,
            _message: message,
            _close: close,
        })
    }
}

pub async fn sleep(milliseconds: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds);
        }
    });
    let _ = JsFuture::from(promise).await;
}

pub fn fresh_id() -> String {
    let now = js_sys::Date::now() as u64;
    let noise = (js_sys::Math::random() * 9_007_199_254_740_991.0) as u64;
    format!("{now:x}-{noise:x}")
}
