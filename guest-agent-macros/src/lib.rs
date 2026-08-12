use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, Pat, ReturnType, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn json_rpc_method(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);

    match expand_json_rpc_method(function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_json_rpc_method(function: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    if !function.sig.generics.params.is_empty() || function.sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "json_rpc_method functions cannot be generic",
        ));
    }

    if function.sig.constness.is_some()
        || function.sig.unsafety.is_some()
        || function.sig.abi.is_some()
        || function.sig.variadic.is_some()
    {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "const, unsafe, extern, and variadic json_rpc_method functions are not supported",
        ));
    }

    let runtime_crate = crate_name("guest-agent").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("could not locate the `guest-agent` crate: {error}"),
        )
    })?;

    // Always use the package lib path so bin targets in this package resolve too.
    let runtime = match runtime_crate {
        FoundCrate::Itself => quote!(::guest_agent),
        FoundCrate::Name(name) => {
            let identifier = format_ident!("{}", name.replace('-', "_"));

            quote!(::#identifier)
        }
    };

    let function_name = &function.sig.ident;
    let visibility = &function.vis;

    let json_rpc_method_name = LitStr::new(&function_name.to_string(), function_name.span());

    let type_prefix = function_name.to_string().to_upper_camel_case();

    let json_rpc_method_type = format_ident!("{type_prefix}JsonRpcMethod");
    let arguments_type = format_ident!("__{type_prefix}JsonRpcMethodArguments");

    let mut argument_fields = Vec::new();
    let mut argument_names = Vec::new();

    for argument in &function.sig.inputs {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "json_rpc_method functions cannot have a `self` receiver",
            ));
        };

        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "json_rpc_method arguments must use simple identifier patterns",
            ));
        };

        if pattern.subpat.is_some() {
            return Err(syn::Error::new_spanned(
                pattern,
                "json_rpc_method arguments cannot use sub-patterns",
            ));
        }

        let name = &pattern.ident;
        let argument_type = &argument.ty;

        argument_fields.push(quote! {
            #name: #argument_type
        });

        argument_names.push(name);
    }

    let returns_result = matches_result_type(&function.sig.output);
    let invoke = if returns_result {
        quote! {
            match #function_name(
                #(__arguments.#argument_names),*
            ) {
                ::core::result::Result::Ok(value) => match ::serde_json::to_value(value) {
                    ::core::result::Result::Ok(value) => ::core::result::Result::Ok(value),
                    ::core::result::Result::Err(_) => ::core::result::Result::Err(
                        #runtime::rpc_models::RpcError::internal_error(),
                    ),
                },
                ::core::result::Result::Err(error) => ::core::result::Result::Err(
                    #runtime::rpc_models::RpcError::from(error)
                ),
            }
        }
    } else {
        quote! {
            match ::serde_json::to_value(#function_name(
                #(__arguments.#argument_names),*
            )) {
                ::core::result::Result::Ok(value) => ::core::result::Result::Ok(value),
                ::core::result::Result::Err(_) => ::core::result::Result::Err(
                    #runtime::rpc_models::RpcError::internal_error(),
                ),
            }
        }
    };

    Ok(quote! {
        #function

        #[doc(hidden)]
        #[derive(
            ::serde::Deserialize,
        )]
        struct #arguments_type {
            #(#argument_fields),*
        }

        #[doc(hidden)]
        #visibility struct #json_rpc_method_type;

        impl #runtime::registry::JsonRpcMethod for #json_rpc_method_type {
            const NAME: &'static str = #json_rpc_method_name;

            fn call(
                arguments: ::serde_json::Value,
            ) -> #runtime::rpc_models::Result<::serde_json::Value> {
                let __arguments: #arguments_type = match ::serde_json::from_value(arguments) {
                    ::core::result::Result::Ok(args) => args,
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            #runtime::rpc_models::RpcError::invalid_params(),
                        );
                    }
                };

                #invoke
            }
        }

        ::inventory::submit! {
            #runtime::registry::JsonRpcMethodRegistration {
                name: <#json_rpc_method_type as #runtime::registry::JsonRpcMethod>::NAME,
                call:
                    <#json_rpc_method_type as #runtime::registry::JsonRpcMethod>::call,
            }
        }
    })
}

fn matches_result_type(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };
    let Type::Path(path) = ty.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Result")
}
