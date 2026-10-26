//! The commands of the code map (`SPC c`, T2.7i.10) on the language
//! servers' features: definition, declaration, implementation, type
//! definition and references, symbols, documentation, rename, code
//! actions and their edits, problems, inlay hints, and a server's own
//! commands and status. Registered with the other built-in commands
//! (`crate::builtin::commands`).

use crate::command::{Command, CommandError, CommandResult, EditorContext, Request, cmd, request};

/// The commands of language servers (D57, T3.8.2), for files a language
/// plugin serves; the answers arrive later, through `lsp::take_outcomes`.
pub(crate) fn code_commands() -> Vec<Command> {
    use crate::lsp::Kind;
    fn ask(ctx: &mut EditorContext<'_>, kind: Kind) -> CommandResult {
        crate::lsp::request(ctx.doc()?, kind).map_err(CommandError::new)
    }
    fn places(
        ctx: &mut EditorContext<'_>,
        title: &str,
        places: Vec<crate::lsp::Place>,
    ) -> CommandResult {
        if places.is_empty() {
            ctx.messages.push(crate::tr!("lsp-no-problems"));
            return Ok(());
        }
        let _ = title;
        request(ctx, Request::Choose(crate::lsp::place_items(&places)))
    }
    let server = Some("hasLanguageServer");
    vec![
        cmd(
            "code.documentation",
            "Show Documentation",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::Hover),
        ),
        cmd(
            "code.definition",
            "Go to Definition",
            "Code",
            &["f12"],
            None,
            |ctx, _| ask(ctx, Kind::Definition),
        ),
        cmd(
            "code.declaration",
            "Go to Declaration",
            "Code",
            &[],
            server,
            |ctx, _| ask(ctx, Kind::Declaration),
        ),
        cmd(
            "code.typeDefinition",
            "Go to Type Definition",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::TypeDefinition),
        ),
        cmd(
            "code.implementation",
            "Go to Implementations",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::Implementation),
        ),
        cmd(
            "code.references",
            "Find References",
            "Code",
            &["shift+f12"],
            None,
            |ctx, _| ask(ctx, Kind::References),
        ),
        cmd(
            "code.symbols",
            "Go to Symbol in Document",
            "Code",
            &[],
            server,
            |ctx, _| ask(ctx, Kind::Symbols),
        ),
        cmd(
            "code.toggleInlayHints",
            "Toggle Inlay Hints",
            "Code",
            &[],
            None,
            |ctx, _| {
                ctx.messages.push(if crate::lsp::toggle_inlay_hints() {
                    crate::tr!("lsp-inlay-hints-shown")
                } else {
                    crate::tr!("lsp-inlay-hints-hidden")
                });
                Ok(())
            },
        ),
        cmd(
            "code.problems",
            "List Problems",
            "Code",
            &[],
            None,
            |ctx, _| {
                let path = ctx.doc()?.meta.path.clone().unwrap_or_default();
                places(ctx, "Problems", crate::lsp::problems_of(Some(&path)))
            },
        ),
        cmd(
            "code.allProblems",
            "List Problems of Open Files",
            "Code",
            &[],
            None,
            |ctx, _| places(ctx, "Problems", crate::lsp::all_problems()),
        ),
        // A server's own requests (the plugin's `requests`): one command
        // each, shown where the document's server has the request.
        cmd(
            "code.expandMacro",
            "Expand Macro",
            "Code",
            &[],
            Some("server:code.expandMacro"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.expandMacro")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.openDocs",
            "Open Documentation in the Browser",
            "Code",
            &[],
            Some("server:code.openDocs"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.openDocs").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.parentModule",
            "Go to Parent Module",
            "Code",
            &[],
            Some("server:code.parentModule"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.parentModule")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.openManifest",
            "Go to Project File",
            "Code",
            &[],
            Some("server:code.openManifest"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.openManifest")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.reloadProject",
            "Reload Project",
            "Code",
            &[],
            Some("server:code.reloadProject"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.reloadProject")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.joinLines",
            "Join Lines (Language Server)",
            "Code",
            &[],
            Some("server:code.joinLines"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.joinLines").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.moveItemUp",
            "Move Item Up",
            "Code",
            &[],
            Some("server:code.moveItemUp"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.moveItemUp").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.moveItemDown",
            "Move Item Down",
            "Code",
            &[],
            Some("server:code.moveItemDown"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.moveItemDown")
                    .map_err(CommandError::new)
            },
        ),
        // A query and its replacement, asked one after the other where the
        // server's request has them (rust-analyzer's `foo($a) ==>>
        // bar($a)`), the edits offered before anything changes.
        cmd(
            "code.structuralReplace",
            "Structural Search and Replace",
            "Code",
            &[],
            Some("server:code.structuralReplace"),
            |ctx, args| {
                let d = ctx.doc()?;
                let asked = crate::lsp::request_inputs(d, "code.structuralReplace")
                    .into_iter()
                    .find(|n| args.get(*n).and_then(serde_json::Value::as_str).is_none());
                match asked {
                    Some(n) => request(
                        ctx,
                        Request::Ask {
                            command: "code.structuralReplace".into(),
                            args: args.clone(),
                            arg: n.into(),
                        },
                    ),
                    None => crate::lsp::server_request_with(d, "code.structuralReplace", args)
                        .map_err(CommandError::new),
                }
            },
        ),
        cmd(
            "code.restartServer",
            "Restart Language Server",
            "Code",
            &[],
            server,
            |ctx, _| {
                let m = crate::lsp::restart(ctx.doc()?).map_err(CommandError::new)?;
                ctx.messages.push(m);
                Ok(())
            },
        ),
        cmd(
            "code.serverStatus",
            "Language Server Status",
            "Code",
            &[],
            None,
            |ctx, _| {
                let mut lines = crate::lsp::report();
                if let Ok(d) = ctx.doc()
                    && let Some(s) = crate::lsp::describe(d)
                {
                    lines.insert(0, s);
                }
                for p in crate::languages::problems() {
                    lines.push(p);
                }
                ctx.messages.push(if lines.is_empty() {
                    crate::tr!("lsp-none-running")
                } else {
                    lines.join(" · ")
                });
                Ok(())
            },
        ),
        cmd(
            "code.rename",
            "Rename Symbol",
            "Code",
            &[],
            None,
            |ctx, args| match args["name"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                Some(name) => crate::lsp::rename(ctx.doc()?, name).map_err(CommandError::new),
                None => request(
                    ctx,
                    Request::Ask {
                        command: "code.rename".into(),
                        args: serde_json::json!({}),
                        arg: "name".into(),
                    },
                ),
            },
        ),
        cmd(
            "code.actions",
            "Code Actions",
            "Code",
            &[],
            None,
            |ctx, _| crate::lsp::code_actions(ctx.doc()?).map_err(CommandError::new),
        ),
        cmd(
            "code.applyEdit",
            "Apply Language Server Edit",
            "Code",
            &[],
            None,
            |ctx, args| {
                let m = crate::lsp::apply_plan(args["id"].as_u64().unwrap_or(0))
                    .map_err(CommandError::new)?;
                ctx.messages.push(m);
                Ok(())
            },
        ),
        cmd(
            "code.dropEdit",
            "Drop Language Server Edit",
            "Code",
            &[],
            None,
            |_ctx, args| {
                crate::lsp::drop_plan(args["id"].as_u64().unwrap_or(0));
                Ok(())
            },
        ),
        cmd(
            "code.runAction",
            "Run Code Action",
            "Code",
            &[],
            None,
            |ctx, args| {
                let m = crate::lsp::run_offer(args["id"].as_u64().unwrap_or(0))
                    .map_err(CommandError::new)?;
                if !m.is_empty() {
                    ctx.messages.push(m);
                }
                Ok(())
            },
        ),
        cmd(
            "code.goto",
            "Go to Place",
            "Code",
            &[],
            None,
            |ctx, args| {
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| CommandError::new("No place"))?;
                request(
                    ctx,
                    Request::OpenAt {
                        path: path.to_string(),
                        line: args["line"].as_u64().unwrap_or(1),
                        column: args["column"].as_u64().unwrap_or(0) as usize,
                    },
                )
            },
        ),
    ]
}
