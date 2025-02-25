use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use chrono::Local;
use encoding::all::ISO_8859_1;
use encoding::{DecoderTrap, Encoding};
use log::{debug, error, warn};
use reqwest::{Client, RequestBuilder};
use regex::Regex;
use crate::member::error::Error::{CantCreateClient, CantCreateMembersFile, CantCreateMembersFileFolder, CantExportList, CantLoadListOnServer, CantPrepareListForExport, CantReadMembersDownloadResponse, CantReadPageContent, CantWriteMembersFile, ConnectionFailed, ConnectionFailedBecauseOfServer, NoCredentials, NoDownloadLink, WrongEncoding, WrongRegex};
use crate::member::{MEMBERS_FILE_FOLDER, Result};
use crate::member::file_details::FileDetails;
use crate::tools::{log_error_and_return, log_message, log_message_and_return};

const URL_DOMAIN: &str = "https://www.leolagrange-fileo.org";

struct Credentials {
    login: String,
    password: String,
}

impl Credentials {
    pub fn new(login: String, password: String) -> Self {
        Self { login, password }
    }
}

fn create_dir() -> Result<()> {
    let err_message = format!("Can't create MEMBERS_FILE_FOLDER `{MEMBERS_FILE_FOLDER}`.");
    let err_mapper = log_message_and_return(
        &err_message,
        CantCreateMembersFileFolder,
    );
    std::fs::create_dir_all(MEMBERS_FILE_FOLDER).map_err(err_mapper)?;

    Ok(())
}

pub async fn download_members_list() -> Result<FileDetails> {
    create_dir()?;

    let client = build_client()?;
    connect(&client).await?;
    load_list_into_server_session(&client).await?;
    let download_url = prepare_list_for_export(&client).await?;
    export_list(&client, &download_url).await
}

fn build_client() -> Result<Client> {
    reqwest::ClientBuilder::new()
        .cookie_store(true)
        .build()
        .map_err(log_message_and_return("Can't build HTTP client.", CantCreateClient))
}

fn retrieve_arg<'a>(arg: &'a str, arg_names: &[&str]) -> Option<&'a str> {
    for arg_name in arg_names {
        let arg_prefix = format!("{arg_name}=");
        if arg.starts_with(&arg_prefix) {
            return arg.split_once("=").map(|(_, l)| l);
        }
    }

    None
}

fn retrieve_login_and_password(args: &Vec<String>) -> (Option<&str>, Option<&str>) {
    let mut login = None;
    let mut password = None;
    for arg in args {
        let arg = arg.trim();
        if let Some(new_login) = retrieve_arg(arg, &["--login", "-l"]) {
            login = Some(new_login);
        }
        if let Some(new_password) = retrieve_arg(arg, &["--password", "-p"]) {
            password = Some(new_password);
        }
    }

    (login, password)
}

fn retrieve_credentials() -> Result<Credentials> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        warn!("Args don't contain login or password. It won't be possible to retrieve the members list.");
        Err(NoCredentials)
    } else {
        let (login, password) = retrieve_login_and_password(&args);

        if let (Some(l), Some(p)) = (login, password) {
            Ok(Credentials::new(l.to_owned(), p.to_owned()))
        } else {
            warn!("Args don't contain login or password. It won't be possible to retrieve the members list.");
            Err(NoCredentials)
        }
    }
}

async fn connect(client: &Client) -> Result<()> {
    let request = prepare_request_for_connection(client)?;
    match request
        .send()
        .await {
        Ok(response) => {
            let status = response.status();
            if status.is_success() || status.is_redirection() {
                Ok(())
            } else {
                error!("Connection failed because of status {status}...");
                Err(ConnectionFailed)
            }
        }
        Err(e) => {
            log_message("Connection failed...")(e);
            Err(ConnectionFailedBecauseOfServer)
        }
    }
}

fn prepare_request_for_connection(client: &Client) -> Result<RequestBuilder> {
    let credentials = retrieve_credentials()?;

    let url = format!("{URL_DOMAIN}/page.php");
    let arguments = [
        ("Action", "connect_user"),
        ("requestForm", "formConnecter"),
        ("login", credentials.login.as_str()),
        ("password", credentials.password.as_str())
    ];
    let body = format_arguments_into_body(&arguments);
    let request = client.post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body);
    Ok(request)
}

async fn load_list_into_server_session(client: &Client) -> Result<()> {
    let request = prepare_request_for_loading_list_into_server_session(client);
    match request
        .send()
        .await {
        Ok(_) => {
            debug!("List loaded on server.");
            Ok(())
        }
        Err(e) => {
            log_message_and_return("The server couldn't load the list.", CantLoadListOnServer)(e);
            Err(CantLoadListOnServer)
        }
    }
}

fn prepare_request_for_loading_list_into_server_session(client: &Client) -> RequestBuilder {
    let url = format!("{URL_DOMAIN}/page.php?P=bo/extranet/adhesion/annuaire/index");
    let arguments = [
        ("Action", "adherent_filtrer"),
        ("requestForm", "formFiltrer"),
        ("affich_select_nom", "3"),
        ("affich_text_nom", ""),
        ("affich_select_prenom", "3"),
        ("affich_text_prenom", ""),
        ("affich_select_majeur", ""),
        ("affich_text_numLicence", ""),
        ("affich_text_dateCreationFrom", ""),
        ("affich_text_dateCreationTo", ""),
        ("affich_text_dateDebut", ""),
        ("affich_text_dateFin", ""),
        ("affich_text_dateSaisieDebut", ""),
        ("affich_text_dateSaisieFin", ""),
        ("affich_radio_statut", ""),
        ("affich_select_regionStructure", ""),
        ("affich_select_departementStructure", ""),
        ("affich_select_code", "2"),
        ("affich_text_code", ""),
        ("affich_fixed_instanceId", "2012"),
        ("affich_radio_structFille", "1"),
        ("affich_select_typeAdhesion", ""),
        ("affich_select_tarif", ""),
        ("affich_select_regle", ""),
        ("affich_select_nomGroupe", "3"),
        ("affich_text_nomGroupe", ""),
    ];
    let body = format_arguments_into_body(&arguments);
    client.post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
}

async fn prepare_list_for_export(client: &Client) -> Result<String> {
    let request = prepare_request_for_preparing_list_for_export(client);
    let response = request
        .send()
        .await
        .map_err(log_message_and_return("Can't export list.", CantPrepareListForExport))?;

    let page_content = response.text().await.map_err(log_error_and_return(CantReadPageContent))?;
    let regex = Regex::new("https://www.leolagrange-fileo.org/clients/fll/telechargements/temp/.*?\\.csv")
        .map_err(log_error_and_return(WrongRegex))?;
    let file_url = regex.find(&page_content).ok_or(NoDownloadLink)?.as_str();
    Ok(file_url.to_owned())
}

fn prepare_request_for_preparing_list_for_export(client: &Client) -> RequestBuilder {
    let url = format!("{URL_DOMAIN}/includer.php?inc=ajax/adherent/adherent_export");
    let arguments = [
        ("requestForm", "formExport"),
        ("export_radio_format", "2"),
        ("option_checkbox_champs[nom]", "nom"),
        ("option_checkbox_champs[prenom]", "prenom"),
        ("option_checkbox_champs[sexe]", "sexe"),
        ("option_checkbox_champs[dateNaissance]", "dateNaissance"),
        ("option_checkbox_champs[age]", "age"),
        ("option_checkbox_champs[numeroLicence]", "numeroLicence"),
        ("option_checkbox_champs[email]", "email"),
        ("option_checkbox_champs[isAdhesionRegle]", "isAdhesionRegle"),
        ("option_checkbox_champs[dateAdhesionFin]", "dateAdhesionFin"),
        ("option_checkbox_champs[expire]", "expire"),
        ("option_checkbox_champs[instanceNom]", "instanceNom"),
        ("option_checkbox_champs[instanceCode]", "instanceCode"),
        ("generation", "2"),
    ];
    let body = format_arguments_into_body(&arguments);
    client.post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
}

async fn export_list(client: &Client, file_url: &str) -> Result<FileDetails> {
    match client.get(file_url).send().await {
        Ok(response) => {
            let date_time = Local::now().date_naive();
            let filename = format!("{MEMBERS_FILE_FOLDER}/members-{}.csv", date_time.format("%Y-%m-%d"));
            let mut file = File::create(&filename).map_err(log_error_and_return(CantCreateMembersFile))?;
            let file_content_as_bytes = response.bytes()
                .await
                .map_err(log_error_and_return(CantReadMembersDownloadResponse))?;
            let file_content = ISO_8859_1
                .decode(file_content_as_bytes.as_ref(), DecoderTrap::Strict)
                .map_err(log_message_and_return("Wrong encoding: expected LATIN-1.", WrongEncoding))?;
            file.write_all(file_content.as_bytes()).map_err(log_error_and_return(CantWriteMembersFile))?;
            Ok(FileDetails::new(date_time, OsString::from(filename)))
        }
        Err(e) => {
            log_message("Can't export list.")(e);
            Err(CantExportList)
        }
    }
}

fn format_arguments_into_body(args: &[(&str, &str)]) -> String {
    args.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join("&")
}

#[cfg(test)]
mod tests {
    use crate::member::download::format_arguments_into_body;

    #[test]
    fn should_format_arguments_into_body() {
        let arguments = [("key1", "value1"), ("key2", "value2")];
        assert_eq!("key1=value1&key2=value2", format_arguments_into_body(&arguments))
    }
}