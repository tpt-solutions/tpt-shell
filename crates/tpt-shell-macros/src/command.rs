// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Expansion of `#[command]`, `commands![]` and `register_commands!` (spec M2).
//!
//! For `#[command(capability = "fs.read")] fn list_dir(path: String) -> Result<Vec<String>, CommandError>`
//! the attribute keeps the function and adds hidden items next to it:
//!
//! - `__tpt_command_list_dir() -> Command`: the description, with the argument schema,
//! - `__tpt_flags_list_dir() -> Flags`: the `ui` and `agent` flags,
//! - `__tpt_handler_list_dir(&Value) -> Result<Value, CommandError>`: reads the typed arguments,
//!   calls the function and serializes the result (`sync` and `async` kinds),
//! - `__tpt_job_list_dir(&Value, &Rc<RefCell<Jobs>>) -> ...`: starts the function on the job
//!   runner and answers `{"job": id}` (`job` kind only),
//! - `__tpt_register_list_dir(&mut Router)`: registers the handler behind its schema.
//!
//! An `async fn` is run to completion by `block_on` on the calling thread. A `job` function
//! takes a `&JobContext` parameter, which is not an argument: it reports progress and checks
//! for cancellation.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, FnArg, Ident, ItemFn, LitBool, LitStr, Pat, PatType, ReturnType, Token, Type};

/// Options accepted by `#[command(...)]`.
#[derive(Default)]
struct Options {
    name: Option<String>,
    capability: Option<String>,
    /// What the call acts on, and the argument that carries it: `target(path = "file")`.
    target: Option<(TargetKind, String)>,
    kind: CommandKind,
    ui: bool,
    agent: bool,
}

/// How the command runs (`kind = "..."`, default `sync`).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CommandKind {
    #[default]
    Sync,
    Async,
    Job,
}

/// The kinds of target a grant's scope can check (see `tpt_shell_ipc::Target`).
#[derive(Clone, Copy)]
enum TargetKind {
    Path,
    Device,
    Url,
    Bytes,
}

impl TargetKind {
    fn from_ident(ident: &Ident) -> Option<Self> {
        match ident.to_string().as_str() {
            "path" => Some(TargetKind::Path),
            "device" => Some(TargetKind::Device),
            "url" => Some(TargetKind::Url),
            "bytes" => Some(TargetKind::Bytes),
            _ => None,
        }
    }
}

/// A flag written bare (`ui`, which means `true`) or with a value (`agent = false`).
fn flag_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<bool> {
    if meta.input.peek(Token![=]) {
        Ok(meta.value()?.parse::<LitBool>()?.value)
    } else {
        Ok(true)
    }
}

fn parse_options(attr: TokenStream2) -> syn::Result<Options> {
    let mut options = Options::default();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            options.name = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("capability") {
            options.capability = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("target") {
            let mut found = None;
            meta.parse_nested_meta(|inner| {
                let kind = TargetKind::from_ident(
                    inner
                        .path
                        .get_ident()
                        .ok_or_else(|| inner.error("expected path, device, url or bytes"))?,
                )
                .ok_or_else(|| inner.error("expected path, device, url or bytes"))?;
                let arg = inner.value()?.parse::<LitStr>()?.value();
                found = Some((kind, arg));
                Ok(())
            })?;
            options.target = found;
            Ok(())
        } else if meta.path.is_ident("kind") {
            let kind = meta.value()?.parse::<LitStr>()?;
            options.kind = match kind.value().as_str() {
                "sync" => CommandKind::Sync,
                "async" => CommandKind::Async,
                "job" => CommandKind::Job,
                "stream" => {
                    return Err(
                        meta.error("stream commands are not built yet; use tpt_shell_ipc::stream")
                    )
                }
                _ => return Err(meta.error("expected kind = \"sync\", \"async\" or \"job\"")),
            };
            Ok(())
        } else if meta.path.is_ident("job") {
            options.kind = CommandKind::Job;
            Ok(())
        } else if meta.path.is_ident("ui") {
            options.ui = flag_value(&meta)?;
            Ok(())
        } else if meta.path.is_ident("agent") {
            options.agent = flag_value(&meta)?;
            Ok(())
        } else {
            Err(meta.error(
                "unknown option; expected `name`, `capability`, `target`, `kind`, `ui` or `agent`",
            ))
        }
    });
    parser.parse2(attr)?;
    Ok(options)
}

/// Whether a type is `Result<..>`, so the generated code can use `?`.
fn is_result(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "Result"),
        _ => false,
    }
}

/// Whether a parameter type is `&JobContext` (or `JobContext`), the job's handle.
fn is_job_context(ty: &Type) -> bool {
    let inner = match ty {
        Type::Reference(reference) => &*reference.elem,
        other => other,
    };
    match inner {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "JobContext"),
        _ => false,
    }
}

/// The closure that reads the call's target from its arguments. Without a `target` option the
/// target is `None`: only an unrestricted grant lets the call run. A missing or wrong-typed
/// argument also gives `None`, so the grant decides, and the handler's own validation refuses
/// the bad argument.
fn target_of(target: Option<&(TargetKind, String)>) -> TokenStream2 {
    let Some((kind, arg)) = target else {
        return quote! { |_: &::tpt_shell::__private::ipc::Value| ::tpt_shell::__private::ipc::Target::None };
    };
    let arg = LitStr::new(arg, proc_macro2::Span::call_site());
    match kind {
        TargetKind::Path => quote! {
            |args: &::tpt_shell::__private::ipc::Value| match args[#arg].as_str() {
                Some(value) => ::tpt_shell::__private::ipc::Target::Path(value.to_string()),
                None => ::tpt_shell::__private::ipc::Target::None,
            }
        },
        TargetKind::Device => quote! {
            |args: &::tpt_shell::__private::ipc::Value| match args[#arg].as_str() {
                Some(value) => ::tpt_shell::__private::ipc::Target::Device(value.to_string()),
                None => ::tpt_shell::__private::ipc::Target::None,
            }
        },
        TargetKind::Url => quote! {
            |args: &::tpt_shell::__private::ipc::Value| match args[#arg].as_str() {
                Some(value) => ::tpt_shell::__private::ipc::Target::Url(value.to_string()),
                None => ::tpt_shell::__private::ipc::Target::None,
            }
        },
        TargetKind::Bytes => quote! {
            |args: &::tpt_shell::__private::ipc::Value| match args[#arg].as_u64() {
                Some(n) => ::tpt_shell::__private::ipc::Target::Bytes(n),
                None => ::tpt_shell::__private::ipc::Target::None,
            }
        },
    }
}

pub fn expand_command(attr: TokenStream2, item: ItemFn) -> syn::Result<TokenStream2> {
    let options = parse_options(attr)?;
    if !item.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.sig.generics,
            "commands cannot be generic",
        ));
    }
    let is_async = item.sig.asyncness.is_some();
    match (options.kind, is_async) {
        (CommandKind::Async, false) => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "kind = \"async\" needs an `async fn`",
            ))
        }
        (CommandKind::Sync, true) => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "an `async fn` needs kind = \"async\"",
            ))
        }
        (CommandKind::Job, true) => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "a job body runs on its own thread; write it as a plain `fn` that takes a `&JobContext`",
            ))
        }
        _ => {}
    }

    let fname = item.sig.ident.clone();
    let command_name = options.name.clone().unwrap_or_else(|| fname.to_string());
    let capability = match &options.capability {
        Some(c) => quote! { ::core::option::Option::Some(#c) },
        None => quote! { ::core::option::Option::None },
    };

    // (identifier, type, argument name on the wire). A job's `&JobContext` is not an argument.
    let mut params: Vec<(Ident, Type, String)> = Vec::new();
    // Every parameter in signature order, as the function call needs them.
    let mut call_args: Vec<TokenStream2> = Vec::new();
    let mut has_job_context = false;
    for arg in &item.sig.inputs {
        match arg {
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new_spanned(
                    receiver,
                    "commands cannot take `self`",
                ));
            }
            FnArg::Typed(PatType { pat, ty, .. }) => match &**pat {
                Pat::Ident(_) if is_job_context(ty) => {
                    if options.kind != CommandKind::Job {
                        return Err(syn::Error::new_spanned(
                            ty,
                            "JobContext is only for a command with kind = \"job\"",
                        ));
                    }
                    has_job_context = true;
                    call_args.push(quote! { ctx });
                }
                Pat::Ident(ident) => {
                    let name = ident.ident.clone();
                    call_args.push(quote! { #name });
                    params.push((ident.ident.clone(), (**ty).clone(), ident.ident.to_string()));
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "command arguments must be plain names",
                    ));
                }
            },
        }
    }
    if options.kind == CommandKind::Job && !has_job_context {
        return Err(syn::Error::new_spanned(
            &item.sig.ident,
            "a job command takes a `&JobContext` parameter, for progress and cancellation",
        ));
    }

    let returns_result = match &item.sig.output {
        ReturnType::Type(_, ty) => is_result(ty.as_ref()),
        ReturnType::Default => false,
    };

    let command_fn = format_ident!("__tpt_command_{}", fname);
    let flags_fn = format_ident!("__tpt_flags_{}", fname);
    let handler_fn = format_ident!("__tpt_handler_{}", fname);
    let job_fn = format_ident!("__tpt_job_{}", fname);
    let register_fn = format_ident!("__tpt_register_{}", fname);

    let kind_path = match options.kind {
        CommandKind::Sync => quote! { ::tpt_shell::__private::ipc::contract::Kind::Sync },
        CommandKind::Async => quote! { ::tpt_shell::__private::ipc::contract::Kind::Async },
        CommandKind::Job => quote! { ::tpt_shell::__private::ipc::contract::Kind::Job },
    };

    let fields = params.iter().map(|(_, ty, name)| {
        quote! {
            ::tpt_shell::__private::ipc::schema::Field {
                name: #name,
                schema: <#ty as ::tpt_shell::__private::ipc::reflect::ToSchema>::schema(),
            }
        }
    });
    let reads = params.iter().map(|(ident, ty, name)| {
        quote! {
            let #ident: #ty = ::tpt_shell::__private::ipc::reflect::__private::arg(args, #name)?;
        }
    });
    // The call. An `async fn` is driven to completion by `block_on`. A job body is called inside
    // the job's closure, where `ctx` is the job context.
    let call_expr = if is_async {
        quote! { ::tpt_shell::__private::ipc::block_on(#fname(#(#call_args),*)) }
    } else {
        quote! { #fname(#(#call_args),*) }
    };
    let call = if returns_result {
        quote! { let out = #call_expr?; }
    } else {
        quote! { let out = #call_expr; }
    };

    let handler = if options.kind == CommandKind::Job {
        let body_call = if returns_result {
            quote! { let out = #fname(#(#call_args),*)?; }
        } else {
            quote! { let out = #fname(#(#call_args),*); }
        };
        quote! {
            #[doc(hidden)]
            pub fn #job_fn(
                args: &::tpt_shell::__private::ipc::Value,
                jobs: &::std::rc::Rc<::std::cell::RefCell<::tpt_shell::__private::ipc::jobs::Jobs>>,
            ) -> ::core::result::Result<::tpt_shell::__private::ipc::Value, ::tpt_shell::__private::ipc::router::CommandError> {
                #(#reads)*
                ::tpt_shell::__private::ipc::jobs::start_shared(jobs, move |ctx: &::tpt_shell::__private::ipc::jobs::JobContext| -> ::core::result::Result<::tpt_shell::__private::ipc::Value, ::tpt_shell::__private::ipc::router::CommandError> {
                    #body_call
                    ::tpt_shell::__private::ipc::reflect::__private::to_value(&out)
                })
            }
        }
    } else {
        quote! {
            #[doc(hidden)]
            pub fn #handler_fn(
                args: &::tpt_shell::__private::ipc::Value,
            ) -> ::core::result::Result<::tpt_shell::__private::ipc::Value, ::tpt_shell::__private::ipc::router::CommandError> {
                #(#reads)*
                #call
                ::tpt_shell::__private::ipc::reflect::__private::to_value(&out)
            }
        }
    };

    // A command with a capability is registered behind its grant. The macro cannot know which
    // part of the arguments it acts on, so the target is `None`: only an unrestricted grant
    // lets it run. A command that acts on a path, device or URL names it with `target(...)`.
    let target_of = target_of(options.target.as_ref());
    let register_call = match (&options.capability, options.kind) {
        (Some(c), CommandKind::Job) => {
            let c = LitStr::new(c, proc_macro2::Span::call_site());
            quote! {
                router.register_job_guarded(command.name, #c, command.args, #target_of, #job_fn);
            }
        }
        (None, CommandKind::Job) => {
            if options.target.is_some() {
                return Err(syn::Error::new_spanned(
                    &item.sig.ident,
                    "target needs a capability: add capability = \"...\"",
                ));
            }
            quote! { router.register_job(command.name, command.args, #job_fn); }
        }
        (Some(c), _) => {
            let c = LitStr::new(c, proc_macro2::Span::call_site());
            quote! {
                router.register_guarded_validated(
                    command.name,
                    #c,
                    command.args,
                    #target_of,
                    #handler_fn,
                );
            }
        }
        (None, _) => {
            if options.target.is_some() {
                return Err(syn::Error::new_spanned(
                    &item.sig.ident,
                    "target needs a capability: add capability = \"...\"",
                ));
            }
            quote! { router.register_validated(command.name, command.args, #handler_fn); }
        }
    };

    let ui = options.ui;
    let agent = options.agent;

    Ok(quote! {
        #item

        #[doc(hidden)]
        pub fn #command_fn() -> ::tpt_shell::__private::ipc::contract::Command {
            ::tpt_shell::__private::ipc::contract::Command {
                name: #command_name,
                kind: #kind_path,
                capability: #capability,
                args: ::tpt_shell::__private::ipc::reflect::__private::struct_schema(
                    ::std::vec![#(#fields),*]
                ),
            }
        }

        #[doc(hidden)]
        pub fn #flags_fn() -> ::tpt_shell::__private::ipc::contract::Flags {
            ::tpt_shell::__private::ipc::contract::Flags { ui: #ui, agent: #agent }
        }

        #handler

        #[doc(hidden)]
        pub fn #register_fn(router: &mut ::tpt_shell::__private::ipc::router::Router) {
            let command = #command_fn();
            #register_call
        }
    })
}

/// `commands![a, b]` builds a `Contract` from commands declared with `#[command]` in this module.
pub fn expand_commands(input: TokenStream2) -> syn::Result<TokenStream2> {
    let names = Punctuated::<Ident, Token![,]>::parse_terminated.parse2(input)?;
    let adds = names.iter().map(|name| {
        let command_fn = format_ident!("__tpt_command_{}", name);
        let flags_fn = format_ident!("__tpt_flags_{}", name);
        quote! {
            contract
                .add_flagged(#command_fn(), #flags_fn())
                .expect("duplicate command name in commands![]");
        }
    });
    Ok(quote! {{
        let mut contract = ::tpt_shell::__private::ipc::contract::Contract::new();
        #(#adds)*
        contract
    }})
}

struct RegisterInput {
    router: Expr,
    names: Punctuated<Ident, Token![,]>,
}

impl Parse for RegisterInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let router: Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        let names = Punctuated::parse_terminated(input)?;
        Ok(RegisterInput { router, names })
    }
}

/// `register_commands!(&mut router, a, b)` registers each command's handler.
pub fn expand_register(input: TokenStream2) -> syn::Result<TokenStream2> {
    let RegisterInput { router, names } = syn::parse2(input)?;
    let registers = names.iter().map(|name| {
        let register_fn = format_ident!("__tpt_register_{}", name);
        quote! { #register_fn(router); }
    });
    Ok(quote! {{
        let router: &mut ::tpt_shell::__private::ipc::router::Router = #router;
        #(#registers)*
    }})
}
