use async_trait::async_trait;
use tokio_modbus::client::{Client, Context};
use tokio_modbus::{Request, Response, Slave};
use tokio_modbus::slave::SlaveContext;

#[derive(Debug)]
pub struct MockContext;

impl MockContext {
    pub fn make() -> Context {
        let client: Box<dyn Client> = Box::new(Self);
        client.into()
    }
}

#[async_trait]
impl SlaveContext for MockContext {
    fn set_slave(&mut self, _: Slave) {

    }
}

#[async_trait]
impl Client for MockContext {
    async fn call(&mut self, request: Request<'_>) -> tokio_modbus::Result<Response> {
        match request {
            Request::ReadHoldingRegisters(a, b) => {
                Ok(Ok(Response::ReadHoldingRegisters(vec![a + b; b as usize])))
            },
            Request::ReadInputRegisters(a, b) => {
                Ok(Ok(Response::ReadInputRegisters(vec![a + b + 1; b as usize])))
            },
            Request::WriteSingleRegister(a, b) => {
                Ok(Ok(Response::WriteSingleRegister(a, b)))
            }
            _ => unimplemented!(),
        }
    }

    async fn disconnect(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}