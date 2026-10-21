// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Expansion of `#[command]`, `commands![]` and `register_commands!` (spec M2).
//!
//! For `#[command(capability = "fs.read")] fn list_dir(path: String) -> Result<Vec<String>, CommandError>`
//! the attribute keeps the function and adds three hidden items next to it:
//!
//! - `__tpt_command_list_dir() -> Command`: the description, with the argument schema,
//! - `__tpt_handler_list_dir(&Value) -> Result<Value, CommandError>`: reads the typed arguments,
//!   calls the function and serializes the result,
//! - `__tpt_register_list_dir(&mut Router)`: registers the handler behind its schema.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, FnArg, Ident, ItemFn, LitStr, Pat, PatType, ReturnType, Token, Type};

/// Options accepted by `#[command(...)]`.
#[derive(Default)]
struct Options {
    name: Option<String>,
    capability: Option<String>,
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
        } else if meta.path.is_ident("kind") {
            let kind = meta.value()?.parse::<LitStr>()?;
            if kind.value() == "sync" {
                Ok(())
            } else {
                Err(meta
                    .error("only kind = \"sync\" is supported; jobs and streams are not built yet"))
            }
        } else {
            Err(meta.error("unknown option; expected `name`, `capability` or `kind`"))
        }
    });
    parser.parse2(attr)?;
    Ok(options)
}

/// Whether a return type is `Result<..>`, so the generated code can use `?`.
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

pub fn expand_command(attr: TokenStream2, item: ItemFn) -> syn::Result<TokenStream2> {
    let options = parse_options(attr)?;
    if !item.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.sig.generics,
            "commands cannot be generic",
        ));
    }
    if item.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            &item.sig,
            "async commands are not built yet; use a synchronous function",
        ));
    }

    let fname = item.sig.ident.clone();
    let command_name = options.name.unwrap_or_else(|| fname.to_string());
    let capability = match &options.capability {
        Some(c) => quote! { ::core::option::Option::Some(#c) },
        None => quote! { ::core::option::Option::None },
    };

    // (identifier, type, argument name on the wire)
    let mut params: Vec<(Ident, Type, String)> = Vec::new();
    for arg in &item.sig.inputs {
        match arg {
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new_spanned(
                    receiver,
                    "commands cannot take `self`",
                ));
            }
            FnArg::Typed(PatType { pat, ty, .. }) => match &**pat {
                Pat::Ident(ident) => {
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

    let returns_result = match &item.sig.output {
        ReturnType::Type(_, ty) => is_result(ty.as_ref()),
        ReturnType::Default => false,
    };

    let command_fn = format_ident!("__tpt_command_{}", fname);
    let handler_fn = format_ident!("__tpt_handler_{}", fname);
    let register_fn = format_ident!("__tpt_register_{}", fname);

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
    let idents = params.iter().map(|(ident, _, _)| ident);
    let call = quote! { #fname(#(#idents),*) };
    let call = if returns_result {
        quote! { let out = #call?; }
    } else {
        quote! { let out = #call; }
    };

    // A command with a capability is registered behind its grant. The macro cannot know which
    // part of the arguments it acts on, so the target is `None`: only an unrestricted grant
    // lets it run. A command that acts on a path, device or URL is registered by hand with a
    // target function (`App::command` in the facade).
    let register_call = match &options.capability {
        Some(c) => {
            let c = LitStr::new(c, proc_macro2::Span::call_site());
            quote! {
                router.register_guarded_validated(
                    command.name,
                    #c,
                    command.args,
                    |_| ::tpt_shell::__private::ipc::Target::None,
                    #handler_fn,
                );
            }
        }
        None => quote! { router.register_validated(command.name, command.args, #handler_fn); },
    };

    Ok(quote! {
        #item

        #[doc(hidden)]
        pub fn #command_fn() -> ::tpt_shell::__private::ipc::contract::Command {
            ::tpt_shell::__private::ipc::contract::Command {
                name: #command_name,
                kind: ::tpt_shell::__private::ipc::contract::Kind::Sync,
                capability: #capability,
                args: ::tpt_shell::__private::ipc::reflect::__private::struct_schema(
                    ::std::vec![#(#fields),*]
                ),
            }
        }

        #[doc(hidden)]
        pub fn #handler_fn(
            args: &::tpt_shell::__private::ipc::Value,
        ) -> ::core::result::Result<::tpt_shell::__private::ipc::Value, ::tpt_shell::__private::ipc::router::CommandError> {
            #(#reads)*
            #call
            ::tpt_shell::__private::ipc::reflect::__private::to_value(&out)
        }

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
        quote! {
            contract
                .add(#command_fn())
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
