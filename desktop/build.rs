//! Põe o ícone dentro do .exe no Windows.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("não consegui pôr o ícone no exe (precisa do rc.exe do Windows SDK)");
    }
}
