use async_trait::async_trait;
use std::collections::HashMap;
use tokio_modbus::client::{Client, Context};
use tokio_modbus::slave::SlaveContext;
use tokio_modbus::{Request, Response, Slave};

#[derive(Debug, Default)]
pub struct MockContext {
    holdings: HashMap<u16, u16>,
}

impl MockContext {
    pub fn make() -> Context {
        let client: Box<dyn Client> = Box::new(Self::default());
        client.into()
    }
}

#[async_trait]
impl SlaveContext for MockContext {
    fn set_slave(&mut self, _: Slave) {}
}

#[async_trait]
impl Client for MockContext {
    async fn call(&mut self, request: Request<'_>) -> tokio_modbus::Result<Response> {
        match request {
            Request::ReadHoldingRegisters(addr, count) => {
                let mut regs = Vec::with_capacity(count as usize);
                for offset in 0..count {
                    let reg_addr = addr + offset;
                    let value = *self.holdings.get(&reg_addr).unwrap_or(&(addr + count));
                    regs.push(value);
                }

                Ok(Ok(Response::ReadHoldingRegisters(regs)))
            }

            Request::ReadInputRegisters(a, b) => Ok(Ok(Response::ReadInputRegisters(vec![
                    a + b + 1;
                    b as usize
                ]))),

            Request::WriteSingleRegister(addr, value) => {
                self.holdings.insert(addr, value);
                Ok(Ok(Response::WriteSingleRegister(addr, value)))
            }

            Request::WriteMultipleRegisters(addr, values) => {
                let quantity = values.len() as u16;
                for (i, v) in values.to_vec().into_iter().enumerate() {
                    self.holdings.insert(addr + i as u16, v);
                }
                Ok(Ok(Response::WriteMultipleRegisters(addr, quantity)))
            }

            _ => unimplemented!(),
        }
    }

    async fn disconnect(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
