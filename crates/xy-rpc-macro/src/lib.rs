use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote};
use std::iter::once;
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Field, FieldMutability, Fields, FieldsNamed, FieldsUnnamed,
    FnArg, GenericArgument, GenericParam, Generics, ItemEnum, ItemTrait, Lifetime, LifetimeParam,
    PathArguments, ReturnType, Token, TraitItem, Type, TypeParamBound, TypeReference, Variant,
    Visibility,
};
/*

#[derive(Debug, Deserialize)]
pub enum RcpTestServiceReturnMsg {
    A(u32),
    B(u32),
    C(String),
}
impl RpcMsg for RcpTestServiceReturnMsg {
    fn index(&self) -> u16 {
        todo!()
    }
}

impl<T> RpcMsgHandler<RcpTestServiceMsg, RcpTestServiceReturnMsg> for T
where
    T: RcpTestService,
{
    fn handle(
        &self,
        msg: RcpTestServiceMsg,
    ) -> impl Future<Output = RcpTestServiceReturnMsg> + Send {
        async {
            match msg {
                RcpTestServiceMsg::A { x } => {
                    let r = self.a(x).await;
                    RcpTestServiceReturnMsg::A(r)
                }
                RcpTestServiceMsg::B { x } => {
                    let r = self.b(x).await;
                    RcpTestServiceReturnMsg::B(r)
                }
                RcpTestServiceMsg::C { x } => {
                    let r = self.c(x).await;
                    RcpTestServiceReturnMsg::C(r)
                }
            }
        }
    }
}
 */

#[proc_macro_attribute]
pub fn rpc_service(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(item as ItemTrait);
    let vis = ast.vis.clone();
    {
        for item in ast.items.iter_mut() {
            if let TraitItem::Fn(f) = item {
                if let Some(_r) = f.sig.asyncness.take() {
                    f.sig.output = match &f.sig.output {
                        ReturnType::Default => parse_quote! {
                            -> impl core::future::Future<Output = ()> + Send
                        },
                        ReturnType::Type(_, ty) => parse_quote! {
                            -> impl core::future::Future<Output = #ty> + Send
                        },
                    };
                }
            }
        }
    }
    let trait_ident = &ast.ident;
    let msg_enum_ident = format_ident!("{}Msg", ast.ident);
    let msg_ref_enum_ident = format_ident!("{}RefMsg", ast.ident);
    let msg_reply_enum_ident = format_ident!("{}ReplyMsg", ast.ident);
    // let msg_reply_ref_enum_ident = format_ident!("{}ReplyRefMsg", ast.ident);
    let _handler_ident = format_ident!("{}Handler", ast.ident);
    let schema_ident = format_ident!("{}Schema", ast.ident);
    let caller_ident = format_ident!("{}Caller", ast.ident);
    let ref_lifetime = Lifetime::new("'a", Span::call_site());
    let (variants, ref_variants, reply_variants) = ast
        .items
        .iter()
        .filter_map(|n| match n {
            syn::TraitItem::Fn(n) => Some(n),
            _ => return None,
        })
        .map(|n| {
            let no_async = n.sig.asyncness.is_none();
            // let found_input_trans_stream = get_input_trans_stream(n);
            let ident = format_ident!("{}", n.sig.ident.to_string().to_case(Case::UpperCamel));
            // match found_input_trans_stream {
            //     None => {
            let (named_fields, named_ref_fields) = n
                .sig
                .inputs
                .iter()
                .filter_map(|n| match n {
                    FnArg::Typed(n) => Some(n),
                    _ => None,
                })
                .map(|n| {
                    (
                        Field {
                            attrs: vec![],
                            vis: Visibility::Inherited,
                            mutability: FieldMutability::None,
                            ident: match n.pat.as_ref() {
                                syn::Pat::Ident(n) => Some(n.ident.clone()),
                                _ => panic!("fm parameters pat: only support named fields"),
                            },
                            colon_token: Default::default(),
                            ty: *n.ty.clone(),
                        },
                        Field {
                            attrs: vec![],
                            vis: Visibility::Inherited,
                            mutability: FieldMutability::None,
                            ident: match n.pat.as_ref() {
                                syn::Pat::Ident(n) => Some(n.ident.clone()),
                                _ => panic!("fm parameters pat: only support named fields"),
                            },
                            colon_token: Default::default(),
                            ty: Type::Reference(TypeReference {
                                and_token: Default::default(),
                                lifetime: Some(ref_lifetime.clone()),
                                mutability: None,
                                elem: Box::new(*n.ty.clone()),
                            }),
                        },
                    )
                })
                .collect();
            let base = Variant {
                attrs: vec![],
                ident: ident.clone(),
                fields: Fields::Named(FieldsNamed {
                    brace_token: Default::default(),
                    named: named_fields,
                }),
                discriminant: None,
            };
            let base_ref = Variant {
                attrs: vec![],
                ident: ident.clone(),
                fields: Fields::Named(FieldsNamed {
                    brace_token: Default::default(),
                    named: named_ref_fields,
                }),
                discriminant: None,
            };
            let base_reply = Variant {
                attrs: vec![],
                ident: ident.clone(),
                fields: Fields::Unnamed({
                    let ty = match &n.sig.output {
                        ReturnType::Default => parse_quote!(()),
                        ReturnType::Type(_, ty) => *ty.clone(),
                    };
                    let ty = get_future_output(no_async, &ty);
                    FieldsUnnamed {
                        paren_token: Default::default(),
                        unnamed: once(Field {
                            attrs: vec![],
                            vis: Visibility::Inherited,
                            mutability: FieldMutability::None,
                            ident: None,
                            colon_token: None,
                            ty,
                        })
                        .collect(),
                    }
                }),
                discriminant: None,
            };
            (base, base_ref, base_reply)
            // }
            // Some((_found_input_trans_stream, found_input_trans_stream_index)) => {
            //     let start_variant = Variant {
            //         attrs: vec![],
            //         ident: ident.clone(),
            //         fields: Fields::Named(FieldsNamed {
            //             brace_token: Default::default(),
            //             named: n
            //                 .sig
            //                 .inputs
            //                 .iter()
            //                 .enumerate()
            //                 .filter(|n| n.0 != found_input_trans_stream_index)
            //                 .filter_map(|(_, n)| match n {
            //                     FnArg::Typed(n) => Some(n),
            //                     _ => None,
            //                 })
            //                 .map(|n| Field {
            //                     attrs: vec![],
            //                     vis: Visibility::Inherited,
            //                     mutability: FieldMutability::None,
            //                     ident: match n.pat.as_ref() {
            //                         syn::Pat::Ident(n) => Some(n.ident.clone()),
            //                         _ => panic!("fm parameters pat: only support named fields"),
            //                     },
            //                     colon_token: Default::default(),
            //                     ty: *n.ty.clone(),
            //                 })
            //                 .collect(),
            //         }),
            //         discriminant: None,
            //     };
            //     let base_reply = Variant {
            //         attrs: vec![],
            //         ident: ident.clone(),
            //         fields: Fields::Unnamed({
            //             let ty = match &n.sig.output {
            //                 ReturnType::Default => parse_quote!(()),
            //                 ReturnType::Type(_, ty) => *ty.clone(),
            //             };
            //             let ty = if no_async {
            //                 let future_output_type = match &ty {
            //                     Type::ImplTrait(type_impl) => {
            //                         type_impl.bounds.iter().find_map(|n| match n {
            //                             TypeParamBound::Trait(t) => {
            //                                 let x = t
            //                                     .path
            //                                     .segments
            //                                     .iter()
            //                                     .find(|n| n.ident == "Future");
            //                                 if let Some(x) = x {
            //                                     let PathArguments::AngleBracketed(args) =
            //                                         &x.arguments
            //                                     else {
            //                                         panic!("invalid return type")
            //                                     };
            //                                     args.args.iter().find_map(|n| match n {
            //                                         GenericArgument::AssocType(a) => {
            //                                             if a.ident == "Output" {
            //                                                 Some(a.ty.clone())
            //                                             } else {
            //                                                 None
            //                                             }
            //                                         }
            //                                         _ => None,
            //                                     })
            //                                 } else {
            //                                     None
            //                                 }
            //                             }
            //                             _ => None,
            //                         })
            //                     }
            //                     _ => None,
            //                 };
            //                 if let Some(rt) = future_output_type {
            //                     rt
            //                 } else {
            //                     parse_quote! {
            //                         <#ty as core::future::Future>::Output
            //                     }
            //                 }
            //             } else {
            //                 ty
            //             };
            //             FieldsUnnamed {
            //                 paren_token: Default::default(),
            //                 unnamed: once(Field {
            //                     attrs: vec![],
            //                     vis: Visibility::Inherited,
            //                     mutability: FieldMutability::None,
            //                     ident: None,
            //                     colon_token: None,
            //                     ty,
            //                 })
            //                 .collect(),
            //             }
            //         }),
            //         discriminant: None,
            //     };
            //     Either::Right(vec![(start_variant, base_reply)].into_iter())
            // }
            // }
        })
        .collect();
    let msg_enum = ItemEnum {
        attrs: parse_quote!(#[derive(Debug,serde::Serialize, serde::Deserialize)]),
        vis: vis.clone(),
        enum_token: Default::default(),
        ident: msg_enum_ident.clone(),
        generics: Default::default(),
        brace_token: Default::default(),
        variants,
    };
    let msg_ref_enum = ItemEnum {
        attrs: parse_quote!(#[derive(Debug,serde::Serialize)]),
        vis: vis.clone(),
        enum_token: Default::default(),
        ident: msg_ref_enum_ident.clone(),
        generics: Generics {
            lt_token: None,
            params: once(GenericParam::Lifetime(LifetimeParam {
                attrs: vec![],
                lifetime: ref_lifetime.clone(),
                colon_token: None,
                bounds: Default::default(),
            }))
            .collect(),
            gt_token: None,
            where_clause: None,
        },
        brace_token: Default::default(),
        variants: ref_variants,
    };
    let msg_reply_enum = ItemEnum {
        attrs: parse_quote!(#[derive(Debug,serde::Serialize, serde::Deserialize)]),
        vis: vis.clone(),
        enum_token: Default::default(),
        ident: msg_reply_enum_ident.clone(),
        generics: Default::default(),
        brace_token: Default::default(),
        variants: reply_variants,
    };

    let (rpc_call_fn, rpc_call_fn_impl, match_expr, msg_info_matches, msg_reply_info_matches, ref_msg_info_matches): (Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>) = ast
      .items
      .iter()
      .filter_map(|n| match n {
         syn::TraitItem::Fn(n) => Some(n),
         _ => return None,
      })
      .enumerate()
      .map(|(i, n)| {
         let id = i + 1;
         let fields: Punctuated<Ident, Token![,]> = n
            .sig
            .inputs
            .iter()
            .filter_map(|n| match n {
               FnArg::Typed(n) => Some(match n.pat.as_ref() {
                  syn::Pat::Ident(n) => n.ident.clone(),
                  _ => panic!("only support named fields"),
               }),
               _ => None,
            })
            .collect();
          let mut rpc_call_fn = n.sig.clone();
          for arg in rpc_call_fn.inputs.iter_mut() {
              if let FnArg::Typed(arg) = arg {
                 arg.ty = Box::new(Type::Reference(TypeReference {
                    and_token: Default::default(),
                    lifetime: Some(ref_lifetime.clone()),
                    mutability: None,
                    elem: Box::new(*arg.ty.clone()),
                 }));
              };
          }
          rpc_call_fn.generics = Generics {
              lt_token: Some(Default::default()),
              params: once(GenericParam::Lifetime(LifetimeParam {
                  attrs: vec![],
                  lifetime: ref_lifetime.clone(),
                  colon_token: None,
                  bounds: Default::default(),
              })).collect(),
              gt_token: Some(Default::default()),
              where_clause: None,
          };
         match rpc_call_fn.output {
            ReturnType::Default => {
               rpc_call_fn = parse_quote! {
                  -> impl core::future::Future<Output = Result<(), xy_rpc::RpcError>> + Send +'static
               }
            }
            ReturnType::Type(_, ty) => {
               let is_async = rpc_call_fn.asyncness.is_some();
               let output_ty = get_future_output(!is_async, &*ty);
               if is_async {
                  rpc_call_fn.asyncness = None;
               }
               rpc_call_fn.output = parse_quote! {
                  -> impl core::future::Future<Output = Result<#output_ty, xy_rpc::RpcError>> + Send +'static
               };
            }
         }
         let fn_ident = &rpc_call_fn.ident;
         let item_name = fn_ident.to_string().to_case(Case::UpperCamel);
         let enum_item_ident = format_ident!("{}", item_name);
         (
            quote! {
                #rpc_call_fn
            },
            quote! {
                #rpc_call_fn {
                    let future = self.call(#msg_ref_enum_ident::#enum_item_ident { #fields });
                    async move {
                        let reply = future.await?;
                        let #msg_reply_enum_ident::#enum_item_ident(reply) = reply.msg else {
                            return Err(xy_rpc::RpcError::InvalidMsg)
                        };
                        Ok(reply)
                    }
                }
            },
            quote! {
                 #msg_enum_ident::#enum_item_ident { #fields } => {
                       let r = self.service.#fn_ident(#fields).await;
                       #msg_reply_enum_ident::#enum_item_ident(r)
                 }
            },
            quote! {
                 #msg_enum_ident::#enum_item_ident { .. } => {
                    xy_rpc::RpcMsgInfo {
                        id: #id as _,
                        name: #item_name
                    }
                 }
            },
            quote! {
                 #msg_reply_enum_ident::#enum_item_ident(_) => {
                    xy_rpc::RpcMsgInfo {
                        id: #id as _,
                        name: #item_name
                    }
                 }
            },
            quote! {
                 #msg_ref_enum_ident::#enum_item_ident { .. } => {
                    xy_rpc::RpcMsgInfo {
                        id: #id as _,
                        name: #item_name
                    }
                 }
            },
         )
      })
      .collect();

    let schema = quote! {
        #[derive(Clone, Debug, Default)]
        #vis struct #schema_ident;
        impl xy_rpc::RpcServiceSchema for #schema_ident
        {
            type Msg = #msg_enum_ident;
            type Reply = #msg_reply_enum_ident;
        }
    };

    let impls = quote! {
        impl<T> xy_rpc::RpcMsgHandler<#schema_ident> for xy_rpc::RpcMsgHandlerWrapper<T>
        where
            T: #trait_ident,
        {
            fn handle(
                &self,
                msg: #msg_enum_ident,
            ) -> impl core::future::Future<Output = #msg_reply_enum_ident> + Send {
                async move {
                    match msg {
                        #(#match_expr)*
                    }
                }
            }
        }
         impl<'a> xy_rpc::RpcRefMsg for #msg_ref_enum_ident<'a> {
               fn info(&self) -> xy_rpc::RpcMsgInfo {
                  match self {
                     #(#ref_msg_info_matches)*
                  }
               }
         }
         impl xy_rpc::RpcRefMsg for #msg_enum_ident {
               fn info(&self) -> xy_rpc::RpcMsgInfo {
                  match self {
                     #(#msg_info_matches)*
                  }
               }
         }
         impl xy_rpc::RpcMsg for #msg_enum_ident {
            type Ref<'a>  = #msg_ref_enum_ident<'a>;
         }
         impl<'a> xy_rpc::RpcRefMsg for &'a #msg_reply_enum_ident {
               fn info(&self) -> xy_rpc::RpcMsgInfo {
                  match self {
                     #(#msg_reply_info_matches)*
                  }
               }
         }
         impl xy_rpc::RpcRefMsg for #msg_reply_enum_ident {
               fn info(&self) -> xy_rpc::RpcMsgInfo {
                  match self {
                     #(#msg_reply_info_matches)*
                  }
               }
         }
         impl xy_rpc::RpcMsg for #msg_reply_enum_ident {
               type Ref<'a>  = &'a #msg_reply_enum_ident;
         }
        #vis trait #caller_ident {
            #(#rpc_call_fn;)*
        }
        impl<CF> #caller_ident for xy_rpc::XyRpcChannel<CF,#schema_ident> where CF: xy_rpc::formats::SerdeFormat {
            #(#rpc_call_fn_impl)*
        }
    };

    quote! {
        #ast
        #schema
        #msg_enum
        #msg_ref_enum
        #msg_reply_enum
        #impls
    }
    .into()
}

fn get_future_output(no_async: bool, ty: &Type) -> Type {
    if no_async {
        let future_output_type = match &ty {
            Type::ImplTrait(type_impl) => type_impl.bounds.iter().find_map(|n| match n {
                TypeParamBound::Trait(t) => {
                    let x = t.path.segments.iter().find(|n| n.ident == "Future");
                    if let Some(x) = x {
                        let PathArguments::AngleBracketed(args) = &x.arguments else {
                            panic!("invalid return type")
                        };
                        args.args.iter().find_map(|n| match n {
                            GenericArgument::AssocType(a) => {
                                if a.ident == "Output" {
                                    Some(a.ty.clone())
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            }),
            _ => None,
        };
        if let Some(rt) = future_output_type {
            rt
        } else {
            parse_quote! {
                <#ty as core::future::Future>::Output
            }
        }
    } else {
        ty.clone()
    }
}
/*
fn get_input_trans_stream(n: &TraitItemFn) -> Option<(&PathSegment, usize)> {
    n.sig.inputs.iter().enumerate().find_map(|(i, n)| {
        let ty = match n {
            FnArg::Typed(n) => n,
            _ => return None,
        };
        let Type::Path(ty) = &*ty.ty else { return None };
        let segment = ty.path.segments.last()?;
        (segment.ident == "TransStream").then_some((segment, i))
    })
}
*/
