use anyhow::{Context, Result};

use crate::cli::args::PlayArgs;
use crate::serve::project;

pub async fn handle_play(args: &PlayArgs) -> Result<()> {
    if args.init {
        let dir = &args.dir;
        project::init_project_dir(dir)
            .context("Failed to initialize .grpctestify project directory")?;
        println!(
            "✨ Initialized .grpctestify/ in {}\n\
             \n\
             \x20 .grpctestify/\n\
             \x20 ├── settings.json        — project settings (in git)\n\
             \x20 ├── .env.example         — template for environments (in git)\n\
             \x20 ├── .gitignore           — keeps *.local, history/, shares/ and reports/ out of git\n\
             \x20 ├── collections/         — .gctf test files (in git)\n\
             \x20 └── history/             — call history (local only, ignored by git)\n\
             \n\
             Next steps:\n\
             \x20 cp .grpctestify/.env.example .grpctestify/.env.staging\n\
             \x20 $EDITOR .grpctestify/.env.staging         # add variables\n\
             \x20 cp .grpctestify/.env.staging .grpctestify/.env.staging.local\n\
             \x20 $EDITOR .grpctestify/.env.staging.local   # add secrets\n\
             \x20 grpctestify play                          # start playground",
            dir.display()
        );
        return Ok(());
    }

    crate::serve::start_play_server(&args.host, args.port, args.dir.clone()).await
}
