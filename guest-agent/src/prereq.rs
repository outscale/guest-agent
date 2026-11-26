use smbioslib::*;

pub fn check() -> Result<(), String> {
    let data = match table_load_from_device() {
        Ok(data) => data,
        Err(e) => {
            log::warn!("Could not check requirments: {e}");
            return Ok(());
        }
    };

    let mut diags = Vec::new();

    if let Some(man) =
        data.find_map(|sys_info: SMBiosSystemInformation| sys_info.manufacturer().ok())
        && man != "3DS Outscale"
    {
        diags.push("OUTSCALE Guest Agent run only on `3DS Outscale VM`");
    }

    if let Some(chip) = data.find_map(|sys_info: SMBiosSystemInformation| sys_info.version().ok())
        && !chip.starts_with("pc-q35-")
    {
        diags.push("OUTSCALE Guest Agent run only on `pc-q35`");
    }

    if !diags.is_empty() {
        Err(diags.join(", "))
    } else {
        Ok(())
    }
}
