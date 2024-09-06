use anyhow::Context;
use std::{fmt::Display, io, path::Path};
use zbus::Connection;
use zbus_polkit::policykit1::{self, CheckAuthorizationFlags};

use crate::zbus::{AptDaemonProxy, AptTransactionProxy};

#[derive(Debug, Clone)]
pub struct Package {
    pub path: String,
    pub name: String,
    pub is_installed: bool,
}

impl Package {
    pub fn new(path: String) -> io::Result<Self> {
        let name = if let Some(os_filename) = Path::new(&path).file_name() {
            match os_filename.to_str() {
                Some(name) => name.to_string(),
                None => String::new(),
            }
        } else {
            String::new()
        };

        Ok(Self {
            path,
            name,
            is_installed: false,
        })
    }
}

pub async fn grant_permissions(package: Package) -> Result<bool, zbus::fdo::Error> {
    let connection = Connection::system().await?;
    let polkit = policykit1::AuthorityProxy::new(&connection).await?;

    let pid = std::process::id();

    let permitted = if pid == 0 {
        true
    } else {
        let subject = zbus_polkit::policykit1::Subject::new_for_owner(pid, None, None)
            .context("could not create policykit1 subject")
            .map_err(zbus_error_from_display)?;

        polkit
            .check_authorization(
                &subject,
                "org.debian.apt.install-file",
                &std::collections::HashMap::new(),
                CheckAuthorizationFlags::AllowUserInteraction.into(),
                "",
            )
            .await
            .context("could not check policykit authorization")
            .map_err(zbus_error_from_display)?
            .is_authorized
    };

    if permitted {
        if let Ok(status) = install_file(&connection, package).await {
            Ok(status)
        } else {
            Err(zbus_error_from_display("Error during installation"))
        }
    } else {
        Err(zbus_error_from_display("Operation not permitted by Polkit"))
    }
}

fn zbus_error_from_display<E: Display>(why: E) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{}", why))
}

async fn install_file(connection: &Connection, package: Package) -> Result<bool, zbus::fdo::Error> {
    if let Ok(proxy) = AptDaemonProxy::new(connection).await {
        if let Ok(path) = proxy.install_file(&package.path, false).await {
            if let Ok(proxy) = AptTransactionProxy::new(connection, path).await {
                if proxy.run().await.is_ok() {
                    return Ok(true);
                } else {
                    return Err(zbus_error_from_display("Error running transaction"));
                }
            }
        }
    }

    Ok(false)
}
