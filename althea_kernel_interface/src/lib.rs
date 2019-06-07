#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use std::env;
use std::io::ErrorKind;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use std::str;

mod check_cron;
mod counter;
mod create_wg_key;
mod delete_tunnel;
mod dns;
mod exit_client_tunnel;
mod exit_server_tunnel;
mod fs_sync;
mod get_neighbors;
mod interface_tools;
mod ip_addr;
mod ip_route;
mod iptables;
mod is_openwrt;
mod link_local_tools;
mod manipulate_uci;
mod open_tunnel;
mod openwrt_ubus;
mod ping_check;
mod set_system_password;
mod setup_wg_if;
mod traffic_control;
mod udp_socket_table;
pub mod wg_iface_counter;

pub use crate::counter::FilterTarget;
pub use crate::create_wg_key::WgKeypair;
pub use crate::exit_server_tunnel::ExitClient;

use failure::Error;
use std::net::AddrParseError;
use std::string::FromUtf8Error;

#[derive(Debug, Fail)]
pub enum KernelInterfaceError {
    #[fail(display = "Runtime Error: {:?}", _0)]
    RuntimeError(String),
    #[fail(display = "No interface by the name: {:?}", _0)]
    NoInterfaceError(String),
    #[fail(display = "Address isn't ready yet: {:?}", _0)]
    AddressNotReadyError(String),
}

impl From<FromUtf8Error> for KernelInterfaceError {
    fn from(e: FromUtf8Error) -> Self {
        KernelInterfaceError::RuntimeError(format!("{:?}", e).to_string())
    }
}

impl From<Error> for KernelInterfaceError {
    fn from(e: Error) -> Self {
        KernelInterfaceError::RuntimeError(format!("{:?}", e).to_string())
    }
}

impl From<AddrParseError> for KernelInterfaceError {
    fn from(e: AddrParseError) -> Self {
        KernelInterfaceError::RuntimeError(format!("{:?}", e).to_string())
    }
}

#[cfg(test)]
lazy_static! {
    pub static ref KI: Box<dyn KernelInterface> = Box::new(TestCommandRunner {
        run_command: Arc::new(Mutex::new(Box::new(|_program, _args| {
            panic!("kernel interface used before initialized");
        })))
    });
}

#[cfg(not(test))]
lazy_static! {
    pub static ref KI: Box<dyn KernelInterface> = Box::new(LinuxCommandRunner {});
}

pub trait CommandRunner {
    fn run_command(&self, program: &str, args: &[&str]) -> Result<Output, Error>;
    fn set_mock(&self, mock: Box<dyn FnMut(String, Vec<String>) -> Result<Output, Error> + Send>);
}

// a quick throwaway function to print arguments arrays so that they can be copy/pasted from logs
fn print_str_array(input: &[&str]) -> String {
    let mut output = String::new();
    for item in input {
        output = output + " " + item;
    }
    output
}

pub struct LinuxCommandRunner;

impl CommandRunner for LinuxCommandRunner {
    fn run_command(&self, program: &str, args: &[&str]) -> Result<Output, Error> {
        let start = Instant::now();
        let output = match Command::new(program).args(args).output() {
            Ok(o) => o,
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    error!("The {:?} binary was not found. Please install a package that provides it. PATH={:?}", program, env::var("PATH"));
                }
                return Err(e.into());
            }
        };

        trace!(
            "Command {} {} returned: {:?}",
            program,
            print_str_array(args),
            output
        );
        if !output.status.success() {
            trace!(
                "Command {} {} returned: an error {:?}",
                program,
                print_str_array(args),
                output
            );
        }
        trace!(
            "command completed in {}s {}ms",
            start.elapsed().as_secs(),
            start.elapsed().subsec_nanos() / 1000000
        );

        if start.elapsed().as_secs() > 5 {
            error!(
                "Command {} {} took more than five seconds to complete!",
                program,
                print_str_array(args)
            );
        } else if start.elapsed().as_secs() > 1 {
            warn!(
                "Command {} {} took more than one second to complete!",
                program,
                print_str_array(args)
            );
        }

        return Ok(output);
    }

    fn set_mock(&self, _mock: Box<dyn FnMut(String, Vec<String>) -> Result<Output, Error> + Send>) {
        unimplemented!()
    }
}

pub struct TestCommandRunner {
    pub run_command:
        Arc<Mutex<Box<dyn FnMut(String, Vec<String>) -> Result<Output, Error> + Send>>>,
}

impl CommandRunner for TestCommandRunner {
    fn run_command(&self, program: &str, args: &[&str]) -> Result<Output, Error> {
        let mut args_owned = Vec::new();
        for a in args {
            args_owned.push(a.to_string())
        }

        (&mut *self.run_command.lock().unwrap())(program.to_string(), args_owned)
    }

    fn set_mock(&self, mock: Box<dyn FnMut(String, Vec<String>) -> Result<Output, Error> + Send>) {
        *self.run_command.lock().unwrap() = mock
    }
}

pub trait KernelInterface: CommandRunner + Sync {}

impl KernelInterface for LinuxCommandRunner {}
impl KernelInterface for TestCommandRunner {}
