fn main() {
    let argument = std::env::args().nth(1);
    if argument.as_deref() == Some("--explain") {
        explain();
        return;
    }
    let error = match argument {
        None => {
            let capability = screenroom_player::capability();
            report(&capability);
            None
        }
        Some(path) => {
            let capability = screenroom_player::capability_at(&path);
            println!("requested:     {path}");
            report(&capability);
            (!capability.available).then(|| capability.reason.clone())
        }
    };

    if let Some(reason) = error {
        eprintln!("the requested libmpv is not usable: {reason}");
        std::process::exit(1);
    }
}

fn explain() {
    println!(
        "bundle layout:   {:?}",
        screenroom_player::BundleLayout::host()
    );
    println!(
        "platform search consults the working directory: {}",
        screenroom_player::system_search_consults_working_directory()
    );
    println!();
    println!("search order (first match wins):");
    for (index, path) in screenroom_player::libmpv_search_plan().iter().enumerate() {
        let exists = if path.exists() { "present" } else { "absent" };
        println!("  {:>2}. {exists:<7} {}", index + 1, path.display());
    }
}

fn report(capability: &screenroom_player::PlayerCapability) {
    println!("available:     {}", capability.available);
    println!("reason:        {}", capability.reason);
    println!(
        "library path:  {}",
        capability.library_path.as_deref().unwrap_or("-")
    );
    println!(
        "api version:   {}",
        capability.api_version.as_deref().unwrap_or("-")
    );
    println!("bridge:        {:?}", screenroom_player::render_bridge());
}
