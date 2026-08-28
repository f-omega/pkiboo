//! Opt-in Magic Wormhole image receiver for paper-share ingestion.
use crate::ui::{Task, TaskStarterExt};
use futures::future::pending;
use magic_wormhole::{MailboxConnection, Wormhole, transfer, transit};
use std::error::Error;

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const RECOVERY_PAGE: &str = "https://f-omega.github.io/pkiboo/recover.html";
const IMAGE_BUNDLE_MAGIC: &[u8] = b"PKIBOO-IMAGES\0";

pub struct ReceivedImage {
    pub filename: String,
    pub bytes: Vec<u8>,
}

pub struct ReceivedImages(pub Vec<ReceivedImage>);

pub async fn receive_image<T: Task>(parent: T) -> Result<ReceivedImages, Box<dyn Error>> {
    parent
        .task("Receive paper images with Magic Wormhole".into(), async |task| {
            let mailbox = MailboxConnection::create(transfer::APP_CONFIG, 3).await?;
            task.set_message(format!(
                "Open {RECOVERY_PAGE}#code={} to send images",
                mailbox.code()
            ))
            .await;
            let wormhole = Wormhole::connect(mailbox).await?;
            let relay = transit::RelayHint::from_urls(
                None,
                [transit::DEFAULT_RELAY_SERVER.parse()?],
            )?;
            let Some(request) = transfer::request_file(
                wormhole,
                vec![relay],
                transit::Abilities::ALL,
                pending(),
            )
            .await?
            else {
                return Err("Magic Wormhole image transfer was cancelled".into());
            };
            let filename = crate::cli_common::untrusted_terminal_label(&request.file_name(), 255);
            let size = request.file_size();
            if size > MAX_IMAGE_BYTES {
                request.reject().await?;
                return Err(format!(
                    "Magic Wormhole image {filename:?} is {size} bytes; maximum is {MAX_IMAGE_BYTES}"
                )
                .into());
            }
            task.set_message(format!("Receiving image {filename} ({size} bytes)")).await;
            let mut bytes = Vec::with_capacity(size as usize);
            let mut output = futures::io::Cursor::new(&mut bytes);
            request.accept(|_| {}, |_, _| {}, &mut output, pending()).await?;
            drop(output);
            decode_image_bundle(filename, bytes)
        })
        .await
}

fn decode_image_bundle(filename: String, bytes: Vec<u8>) -> Result<ReceivedImages, Box<dyn Error>> {
    if !bytes.starts_with(IMAGE_BUNDLE_MAGIC) {
        return Ok(ReceivedImages(vec![ReceivedImage { filename, bytes }]));
    }
    let mut position = IMAGE_BUNDLE_MAGIC.len();
    let count = read_u32(&bytes, &mut position)? as usize;
    if count == 0 || count > 256 {
        return Err(format!("Paper-image bundle contains an invalid image count ({count})").into());
    }
    let mut images = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = read_u32(&bytes, &mut position)? as usize;
        let size = read_u64(&bytes, &mut position)? as usize;
        let name_end = position.checked_add(name_len).ok_or("Image name length overflow")?;
        let data_end = name_end.checked_add(size).ok_or("Image length overflow")?;
        if data_end > bytes.len() {
            return Err("Paper-image bundle ended before an image was complete".into());
        }
        let name = String::from_utf8_lossy(&bytes[position..name_end]);
        images.push(ReceivedImage {
            filename: crate::cli_common::untrusted_terminal_label(&name, 255),
            bytes: bytes[name_end..data_end].to_vec(),
        });
        position = data_end;
    }
    if position != bytes.len() {
        return Err("Paper-image bundle contains trailing data".into());
    }
    Ok(ReceivedImages(images))
}

fn read_u32(bytes: &[u8], position: &mut usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(read_array(bytes, position)?))
}

fn read_u64(bytes: &[u8], position: &mut usize) -> Result<u64, Box<dyn Error>> {
    Ok(u64::from_le_bytes(read_array(bytes, position)?))
}

fn read_array<const N: usize>(bytes: &[u8], position: &mut usize) -> Result<[u8; N], Box<dyn Error>> {
    let end = position.checked_add(N).ok_or("Paper-image bundle length overflow")?;
    let value = bytes.get(*position..end).ok_or("Paper-image bundle ended unexpectedly")?.try_into()?;
    *position = end;
    Ok(value)
}
