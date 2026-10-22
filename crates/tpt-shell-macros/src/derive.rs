// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `#[derive(Contract)]` (spec §8): gives a struct or a unit-variant enum the argument schema
//! that the contract needs, by implementing `ToSchema`.
//!
//! - A struct becomes `Schema::Struct` with one field per named field, in declaration order.
//! - An enum whose variants are all unit variants becomes `Schema::Enum` of the variant names.
//!
//! Bounds go on fields: `#[contract(min = 0, max = 9)]` for numbers, `max_len = 16` for strings,
//! `max_items = 4` for lists and maps, and `chars = "abc-"` for a string's allowed characters.
//! A bound that does not fit the field's type (`max_len` on a number, say) makes building the
//! schema panic, which the first test or start-up of the app shows, so it is not silently lost.
//!
//! The derive does not read serde attributes. A `#[serde(...)]` attribute on the type, a field
//! or a variant is refused, because the names it would change would not match the schema.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::{Data, DeriveInput, Fields, LitInt, LitStr, Token, Type};

/// Bounds written on one field, each optional.
#[derive(Default)]
struct Bounds {
    min: Option<i64>,
    max: Option<i64>,
    max_len: Option<usize>,
    max_items: Option<usize>,
    chars: Option<String>,
}

/// Support: Full.
pub fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Contract cannot derive a schema for a generic type",
        ));
    }
    refuse_serde(&input.attrs)?;
    let name = &input.ident;
    let body = match &input.data {
        Data::Struct(data) => struct_schema(&data.fields)?,
        Data::Enum(data) => enum_schema(data)?,
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Contract cannot derive a schema for a union",
            ))
        }
    };
    Ok(quote! {
        impl ::tpt_shell::__private::ipc::reflect::ToSchema for #name {
            fn schema() -> ::tpt_shell::__private::ipc::schema::Schema {
                #body
            }
        }
    })
}

fn struct_schema(fields: &Fields) -> syn::Result<TokenStream2> {
    let Fields::Named(named) = fields else {
        return Err(syn::Error::new_spanned(
            fields,
            "Contract needs a struct with named fields",
        ));
    };
    let mut entries = Vec::new();
    for field in &named.named {
        refuse_serde(&field.attrs)?;
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "Contract needs named fields"))?;
        let name = ident.to_string();
        let bounds = bounds_of(&field.attrs)?;
        let schema = field_schema(&field.ty, &bounds)?;
        entries.push(quote! {
            ::tpt_shell::__private::ipc::schema::Field { name: #name, schema: #schema }
        });
    }
    Ok(quote! {
        ::tpt_shell::__private::ipc::reflect::__private::struct_schema(::std::vec![#(#entries),*])
    })
}

/// The schema for one field: the field type's own schema, with the field's bounds applied.
fn field_schema(ty: &Type, bounds: &Bounds) -> syn::Result<TokenStream2> {
    let bounds_tokens = bounds_tokens(bounds);
    Ok(quote! {
        ::tpt_shell::__private::ipc::reflect::__private::bounded(
            <#ty as ::tpt_shell::__private::ipc::reflect::ToSchema>::schema(),
            #bounds_tokens,
        )
    })
}

fn bounds_tokens(b: &Bounds) -> TokenStream2 {
    let opt = |value: Option<TokenStream2>| match value {
        Some(v) => quote! { ::core::option::Option::Some(#v) },
        None => quote! { ::core::option::Option::None },
    };
    let min = opt(b.min.map(|v| quote! { #v }));
    let max = opt(b.max.map(|v| quote! { #v }));
    let max_len = opt(b.max_len.map(|v| quote! { #v }));
    let max_items = opt(b.max_items.map(|v| quote! { #v }));
    let chars = opt(b.chars.as_ref().map(|c| {
        let c = LitStr::new(c, proc_macro2::Span::call_site());
        quote! { #c }
    }));
    quote! {
        ::tpt_shell::__private::ipc::reflect::__private::Bounds {
            min: #min,
            max: #max,
            max_len: #max_len,
            max_items: #max_items,
            chars: #chars,
        }
    }
}

fn enum_schema(data: &syn::DataEnum) -> syn::Result<TokenStream2> {
    let mut names = Vec::new();
    for variant in &data.variants {
        refuse_serde(&variant.attrs)?;
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "Contract can only derive an enum whose variants have no fields",
            ));
        }
        names.push(variant.ident.to_string());
    }
    if names.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "Contract needs an enum with at least one variant",
        ));
    }
    Ok(quote! {
        ::tpt_shell::__private::ipc::schema::Schema::Enum(&[#(#names),*])
    })
}

/// Reads the `#[contract(...)]` bounds of a field. Each bound has one meaning; an unknown key is
/// an error, so a typo does not silently drop a limit.
fn bounds_of(attrs: &[syn::Attribute]) -> syn::Result<Bounds> {
    let mut bounds = Bounds::default();
    for attr in attrs {
        if !attr.path().is_ident("contract") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("min") {
                bounds.min = Some(signed_int(&meta)?);
            } else if meta.path.is_ident("max") {
                bounds.max = Some(signed_int(&meta)?);
            } else if meta.path.is_ident("max_len") {
                bounds.max_len = Some(unsigned_int(&meta)?);
            } else if meta.path.is_ident("max_items") {
                bounds.max_items = Some(unsigned_int(&meta)?);
            } else if meta.path.is_ident("chars") {
                bounds.chars = Some(meta.value()?.parse::<LitStr>()?.value());
            } else {
                return Err(meta.error(
                    "unknown bound; expected `min`, `max`, `max_len`, `max_items` or `chars`",
                ));
            }
            Ok(())
        })?;
    }
    Ok(bounds)
}

fn signed_int(meta: &syn::meta::ParseNestedMeta) -> syn::Result<i64> {
    let input = meta.value()?;
    let negative = input.peek(Token![-]);
    if negative {
        input.parse::<Token![-]>()?;
    }
    let lit: LitInt = input.parse()?;
    let value: i64 = lit.base10_parse()?;
    Ok(if negative { -value } else { value })
}

fn unsigned_int(meta: &syn::meta::ParseNestedMeta) -> syn::Result<usize> {
    let lit: LitInt = meta.value()?.parse()?;
    lit.base10_parse()
}

/// Refuses `#[serde(...)]` on an item. The derive reads field and variant names as written.
fn refuse_serde(attrs: &[syn::Attribute]) -> syn::Result<()> {
    for attr in attrs {
        if attr.path().is_ident("serde") {
            return Err(syn::Error::new_spanned(
                attr.to_token_stream(),
                "Contract does not read serde attributes; use the names as written, without renaming",
            ));
        }
    }
    Ok(())
}
