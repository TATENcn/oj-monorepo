use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Expr, Fields, Ident, ItemStruct, Lit, LitInt, MetaNameValue,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

enum ConfigVal {
    Default(Expr),
    DefaultSecs(u64),
}

impl Parse for ConfigVal {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let meta: MetaNameValue = input.parse()?;
        let ident = meta.path.get_ident().map(|i| i.to_string());

        match ident.as_deref() {
            Some("default") => Ok(ConfigVal::Default(meta.value)),
            Some("default_secs") => match &meta.value {
                Expr::Lit(syn::ExprLit { lit: Lit::Int(lit_int), .. }) => Ok(ConfigVal::DefaultSecs(lit_int.base10_parse()?)),
                _ => Err(syn::Error::new(meta.value.span(), "expected integer literal")),
            },
            _ => Err(syn::Error::new(meta.span(), "expected `default` or `default_secs`")),
        }
    }
}

fn extract_config_attr(attrs: &[Attribute]) -> Option<ConfigVal> {
    for attr in attrs {
        if attr.path().is_ident("config_val") {
            return attr.parse_args::<ConfigVal>().ok();
        }
    }
    None
}

fn default_fn_ident(_struct_name: &Ident, field_name: &Ident) -> Ident {
    format_ident!("__config_default_{}", field_name)
}

fn default_fn_path_str(struct_name: &Ident, fn_name: &Ident) -> String {
    format!("{}::{}", struct_name, fn_name)
}

#[proc_macro_attribute]
pub fn config(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let struct_name = &input.ident;
    let struct_visibility = &input.vis;

    let fields = match &input.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return syn::Error::new(input.span(), "config can only be used on structs with named fields")
                .to_compile_error()
                .into();
        }
    };

    let deserialize_fn_name = format_ident!("__config_deserialize_duration_secs");
    let deserialize_fn_name_str = format!("{}::{}", struct_name, deserialize_fn_name);

    let mut impl_fns = Vec::new();
    let mut field_tokens = Vec::new();
    let mut default_impl_fields = Vec::new();
    let mut has_any_duration = false;

    for field in fields.iter() {
        let field_ident = field.ident.as_ref().unwrap();
        let fn_name = default_fn_ident(struct_name, field_ident);
        let fn_path_str = default_fn_path_str(struct_name, &fn_name);
        let field_type = &field.ty;
        let visibility = &field.vis;

        // Strip `#[config_val(...)]` for contact
        let mut attrs: Vec<Attribute> = field.attrs.iter().filter(|a| !a.path().is_ident("config_val")).cloned().collect();

        match extract_config_attr(&field.attrs) {
            Some(config_val) => {
                match config_val {
                    ConfigVal::Default(expr) => {
                        impl_fns.push(quote! {
                            #[allow(non_snake_case)]
                            fn #fn_name() -> #field_type { #expr }
                        });
                        attrs.push(syn::parse_quote! { #[serde(default = #fn_path_str)] });
                    }
                    ConfigVal::DefaultSecs(n) => {
                        has_any_duration = true;
                        let lit = LitInt::new(&n.to_string(), proc_macro2::Span::call_site());
                        impl_fns.push(quote! {
                            #[allow(non_snake_case)]
                            fn #fn_name() -> ::std::time::Duration { ::std::time::Duration::from_secs(#lit) }
                        });
                        attrs.push(syn::parse_quote! { #[serde(default = #fn_path_str, deserialize_with = #deserialize_fn_name_str)] });
                    }
                }
                default_impl_fields.push(quote! {
                    #field_ident: Self::#fn_name(),
                });
            }
            None => {
                // Inject `#[serde(default)` when no hand-written serde attribute
                if !attrs.iter().any(|a| a.path().is_ident("serde")) {
                    attrs.push(syn::parse_quote! { #[serde(default)] });
                }

                default_impl_fields.push(quote! {
                    #field_ident: Default::default(),
                });
            }
        }

        field_tokens.push(quote! {
            #(#attrs)*
            #visibility #field_ident: #field_type,
        });
    }

    if has_any_duration {
        impl_fns.push(quote! {
            #[allow(non_snake_case)]
            fn #deserialize_fn_name<'de, D>(deserializer: D) -> Result<::std::time::Duration, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let secs: u64 = ::serde::Deserialize::deserialize(deserializer)?;
                Ok(::std::time::Duration::from_secs(secs))
            }
        });
    }

    let generics = &input.generics;
    let where_clause = &generics.where_clause;
    let attrs = &input.attrs;

    let expanded = quote! {
        #(#attrs)*
        #struct_visibility struct #struct_name #generics #where_clause {
            #(#field_tokens)*
        }

        impl #generics #struct_name #generics #where_clause {
            #(#impl_fns)*
        }

        impl #generics Default for #struct_name #generics #where_clause {
            fn default() -> Self {
                Self {
                    #(#default_impl_fields)*
                }
            }
        }
    };

    expanded.into()
}
