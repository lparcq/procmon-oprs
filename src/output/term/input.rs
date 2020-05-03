use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use termion::event;
use termion::input::TermRead;

type InputResult = io::Result<event::Event>;

type InputOptionalResult = io::Result<Option<event::Event>>;

pub struct EventChannel {
    chin: mpsc::Receiver<InputResult>,
}

impl EventChannel {
    pub fn new() -> EventChannel {
        let (chout, chin) = mpsc::channel();
        thread::spawn(move || {
            for res in io::stdin().events() {
                if chout.send(res).is_err() {
                    break;
                }
            }
        });
        EventChannel { chin }
    }

    fn disconnected() -> io::Error {
        io::Error::new(io::ErrorKind::ConnectionAborted, "channel disconnected")
    }

    pub fn receive_timeout(&self, timeout: Duration) -> InputOptionalResult {
        match self.chin.recv_timeout(timeout) {
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Err(EventChannel::disconnected()),
            Ok(res) => res.map(Some),
        }
    }
}
