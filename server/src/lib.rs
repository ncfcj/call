//! Biblioteca do `call-server`. O binário (`main.rs`) só lê a configuração e sobe o servidor;
//! os testes de integração em `tests/` montam o mesmo [`routes::router`].

pub mod access;
pub mod auth;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;
