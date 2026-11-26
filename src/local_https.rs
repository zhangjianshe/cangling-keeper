//! Loopback-only HTTPS. Only the leaf key is embedded, never a CA key.
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::{TcpListener, TcpStream}, task::JoinSet};
use tokio_rustls::{TlsAcceptor, server::TlsStream, rustls::{self, pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject}}};

pub(crate) struct Listener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    handshakes: JoinSet<Option<(TlsStream<TcpStream>, SocketAddr)>>,
}
impl Listener {
    pub(crate) async fn bind(address: SocketAddr,inherited:Option<std::net::TcpListener>) -> Result<Self, String> {
        if !address.ip().is_loopback() { return Err("HTTPS 仅允许监听本机回环地址".into()); }
        let certs = CertificateDer::pem_slice_iter(include_bytes!("local-https/cert-chain.pem")).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("local-https/key.pem")).map_err(|e|e.to_string())?;
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions().map_err(|e|e.to_string())?
            .with_no_client_auth().with_single_cert(certs,key).map_err(|e|e.to_string())?;
        let tcp=if let Some(listener)=inherited{TcpListener::from_std(listener).map_err(|e|e.to_string())?}else{TcpListener::bind(address).await.map_err(|e|format!("HTTPS 监听 {address} 失败：{e}；请检查端口占用及低端口权限，HTTP 服务继续运行"))?};
        Ok(Self {tcp, acceptor:TlsAcceptor::from(Arc::new(config)), handshakes:JoinSet::new()})
    }
}
impl axum::serve::Listener for Listener {
    type Io=TlsStream<TcpStream>;
    type Addr=SocketAddr;
    async fn accept(&mut self)->(Self::Io,Self::Addr) {
        loop {tokio::select! {
            result=self.handshakes.join_next(), if !self.handshakes.is_empty()=>{
                if let Some(Ok(Some(connection)))=result{return connection;}
            }
            result=self.tcp.accept(), if self.handshakes.len()<64=>{
                match result {
                    Ok((stream,peer))=>{let acceptor=self.acceptor.clone();self.handshakes.spawn(async move {
                        tokio::time::timeout(Duration::from_secs(5),acceptor.accept(stream)).await.ok()?.ok().map(|tls|(tls,peer))
                    });}
                    Err(_)=>tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        }}
    }
    fn local_addr(&self)->std::io::Result<SocketAddr>{self.tcp.local_addr()}
}
