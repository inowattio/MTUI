use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const MAX_FRAME_LEN: usize = 256;

pub struct EchoCancel<T> {
    inner: T,
    expected: VecDeque<u8>,
    held: Vec<u8>,
    leftover: Vec<u8>,
    frame_done: bool,
    warned: bool,
}

impl<T> EchoCancel<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            expected: VecDeque::with_capacity(MAX_FRAME_LEN),
            held: Vec::new(),
            leftover: Vec::new(),
            frame_done: false,
            warned: false,
        }
    }

    fn filter(&mut self, data: &[u8]) -> Vec<u8> {
        if self.expected.is_empty() {
            return data.to_vec();
        }
        let mut matched = 0;
        while let (Some(&expected), Some(&actual)) = (self.expected.front(), data.get(matched)) {
            if expected != actual {
                if !self.warned {
                    log::warn!(
                        "Local echo is on but the port returned 0x{actual:02X} where the echoed 0x{expected:02X} was expected"
                    );
                    self.warned = true;
                }
                self.expected.clear();
                let mut out = std::mem::take(&mut self.held);
                out.extend_from_slice(data);
                return out;
            }
            self.expected.pop_front();
            matched += 1;
        }
        if self.expected.is_empty() {
            self.held.clear();
            return data[matched..].to_vec();
        }
        self.held.extend_from_slice(data);
        Vec::new()
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for EchoCancel<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if !this.leftover.is_empty() {
                let rest = this
                    .leftover
                    .split_off(this.leftover.len().min(buf.remaining()));
                buf.put_slice(&this.leftover);
                this.leftover = rest;
                return Poll::Ready(Ok(()));
            }
            let before = buf.filled().len();
            ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
            if buf.filled().len() == before {
                return Poll::Ready(Ok(()));
            }
            let data = buf.filled()[before..].to_vec();
            buf.set_filled(before);
            let out = this.filter(&data);
            if out.is_empty() {
                continue;
            }
            this.frame_done = true;
            let fits = out.len().min(buf.remaining());
            buf.put_slice(&out[..fits]);
            this.leftover.extend_from_slice(&out[fits..]);
            return Poll::Ready(Ok(()));
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for EchoCancel<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = ready!(Pin::new(&mut this.inner).poll_write(cx, data))?;
        if this.frame_done {
            this.expected.clear();
            this.held.clear();
            this.frame_done = false;
        }
        this.expected.extend(&data[..written]);
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
        this.frame_done = true;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::EchoCancel;
    use std::collections::VecDeque;
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

    const REQUEST: [u8; 8] = [0x01, 0x03, 0x80, 0x00, 0x00, 0x0A, 0xEC, 0x0D];
    const RESPONSE: [u8; 7] = [0x01, 0x03, 0x02, 0x12, 0x34, 0xB5, 0x33];

    struct Port {
        incoming: VecDeque<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Port {
        fn new(chunks: &[&[u8]]) -> Self {
            Self {
                incoming: chunks.iter().map(|c| c.to_vec()).collect(),
                sent: Vec::new(),
            }
        }
    }

    impl AsyncRead for Port {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let incoming = &mut self.get_mut().incoming;
            if let Some(mut chunk) = incoming.pop_front() {
                let take = chunk.len().min(buf.remaining());
                let rest = chunk.split_off(take);
                buf.put_slice(&chunk);
                if !rest.is_empty() {
                    incoming.push_front(rest);
                }
            }
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for Port {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.get_mut().sent.extend_from_slice(data);
            Poll::Ready(Ok(data.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    async fn exchange(chunks: &[&[u8]]) -> Vec<u8> {
        let mut port = EchoCancel::new(Port::new(chunks));
        port.write_all(&REQUEST).await.unwrap();
        let mut out = Vec::new();
        port.read_to_end(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn drops_the_echo_that_arrives_with_the_response() {
        let mut wire = REQUEST.to_vec();
        wire.extend(RESPONSE);
        assert_eq!(exchange(&[&wire]).await, RESPONSE);
    }

    #[tokio::test]
    async fn drops_an_echo_split_over_several_reads() {
        let wire: Vec<&[u8]> = vec![&REQUEST[..3], &REQUEST[3..6], &REQUEST[6..], &RESPONSE];
        assert_eq!(exchange(&wire).await, RESPONSE);
    }

    #[tokio::test]
    async fn keeps_the_response_bytes_that_share_a_read_with_the_echo_tail() {
        let mut tail = REQUEST[5..].to_vec();
        tail.extend(&RESPONSE[..4]);
        let wire: Vec<&[u8]> = vec![&REQUEST[..5], &tail, &RESPONSE[4..]];
        assert_eq!(exchange(&wire).await, RESPONSE);
    }

    #[tokio::test]
    async fn passes_everything_through_once_the_bytes_stop_matching() {
        let mut wire = REQUEST[..2].to_vec();
        wire.push(0xFF);
        wire.extend(RESPONSE);
        assert_eq!(exchange(&[&wire]).await, wire);
    }

    #[tokio::test]
    async fn an_identical_response_is_not_mistaken_for_echo_when_the_echo_already_passed() {
        let write = [0x01, 0x06, 0x81, 0x80, 0x42, 0x42, 0x10, 0x8F];
        let mut wire = write.to_vec();
        wire.extend(write);
        let mut port = EchoCancel::new(Port::new(&[&wire]));
        port.write_all(&write).await.unwrap();
        let mut out = Vec::new();
        port.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, write);
    }

    #[tokio::test]
    async fn a_new_request_forgets_an_echo_that_never_came() {
        let other = [0x01, 0x03, 0x00, 0x10, 0x00, 0x01, 0x85, 0xCF];
        let reply = [0x01, 0x03, 0x02, 0xAA, 0xBB, 0x8D, 0xE9];
        let mut second = REQUEST.to_vec();
        second.extend(RESPONSE);
        let mut port = EchoCancel::new(Port::new(&[&reply, &second]));

        port.write_all(&other).await.unwrap();
        port.flush().await.unwrap();
        let mut buf = [0u8; 7];
        port.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, reply, "no echo: the reply passes through as is");

        port.write_all(&REQUEST).await.unwrap();
        let mut out = Vec::new();
        port.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, RESPONSE, "only the latest request is treated as echo");
    }

    #[tokio::test]
    async fn without_echo_a_reply_sharing_the_request_prefix_stays_intact() {
        let reply = [0x01, 0x03, 0x02, 0xAA, 0xBB, 0x8D, 0xE9];
        assert_eq!(exchange(&[&reply]).await, reply);
        assert_eq!(exchange(&[&reply[..2], &reply[2..]]).await, reply);
    }

    #[tokio::test]
    async fn a_timed_out_request_does_not_poison_the_next_frame() {
        let mut wire = REQUEST.to_vec();
        wire.extend(RESPONSE);
        let mut port = EchoCancel::new(Port::new(&[&wire]));
        let other = [0x01, 0x03, 0x00, 0x10, 0x00, 0x01, 0x85, 0xCF];
        port.write_all(&other).await.unwrap();
        port.flush().await.unwrap();
        port.write_all(&REQUEST).await.unwrap();
        port.flush().await.unwrap();
        let mut out = Vec::new();
        port.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, RESPONSE);
    }

    #[tokio::test]
    async fn writes_reach_the_port_unchanged() {
        let mut port = EchoCancel::new(Port::new(&[]));
        port.write_all(&REQUEST).await.unwrap();
        assert_eq!(port.inner.sent, REQUEST);
    }
}
