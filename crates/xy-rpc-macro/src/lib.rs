use convert_case::{Case, Casing};
use core::iter::once;
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{
    Field, FieldMutability, Fields, FieldsNamed, FnArg, GenericArgument, GenericParam, Generics,
    ItemStruct, ItemTrait, Lifetime, LifetimeParam, PatType, PathArguments, PathSegment,
    ReturnType, Token, TraitItem, TraitItemFn, Type, TypeParamBound, TypeReference,
    parse_macro_input, parse_quote,
};

#[proc_macro_attribute]
pub fn rpc_service(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(item as ItemTrait);
    let vis = ast.vis.clone();
    {
        for item in ast.items.iter_mut() {
            if let TraitItem::Fn(f) = item {
                let is_async = f.sig.asyncness.take().is_some();
                f.sig.output = match &f.sig.output {
                    ReturnType::Default => {
                        if !is_async {
                            panic!("rpc fn return type must impl Future");
                        }
                        parse_quote! {
                           -> impl core::future::Future<Output = ()> + xy_rpc::maybe_send::MaybeSend
                        }
                    }
                    ReturnType::Type(_, ty) => {
                        let output_ty = get_future_output(!is_async, &*ty);
                        parse_quote! {
                           -> impl core::future::Future<Output = #output_ty> + xy_rpc::maybe_send::MaybeSend
                        }
                    }
                };
            }
        }
    }
    let trait_ident = &ast.ident;
    // let mod_ident = format_ident!("{}", ast.ident.to_string().to_case(Case::Snake));
    let variant_ident = format_ident!("{}Variant", ast.ident);
    // let msg_ref_enum_ident = format_ident!("{}RefMsg", ast.ident);
    // let msg_reply_enum_ident = format_ident!("{}ReplyMsg", ast.ident);
    // let msg_reply_ref_enum_ident = format_ident!("{}ReplyRefMsg", ast.ident);
    let _handler_ident = format_ident!("{}Handler", ast.ident);
    let schema_ident = format_ident!("{}Schema", ast.ident);
    let wasm_channel_proxy = format_ident!("{}ChannelProxy", ast.ident);
    let caller_ident = format_ident!("{}Caller", ast.ident);
    let ref_lifetime = Lifetime::new("'a", Span::call_site());

    struct RpcItem {
        msg_struct: ItemStruct,
        msg_ref_struct: ItemStruct,
        // reply_type: Type,
        // stream_item_type: Option<Type>,
        name: String,
        index: usize,
    }

    let rpc_items: Vec<_> = ast
        .items
        .iter()
        .filter_map(|n| match n {
            syn::TraitItem::Fn(n) => Some(n),
            _ => return None,
        })
        .enumerate()
        .map(|(index, n)| {
            // let no_async = n.sig.asyncness.is_none();
            let found_input_trans_stream = get_input_trans_stream(n);
            let args = n.sig.inputs.iter().enumerate().filter(|n| {
                !found_input_trans_stream
                    .as_ref()
                    .is_some_and(|s| s.3 == n.0)
            });
            let name = n.sig.ident.to_string().to_case(Case::UpperCamel);
            let ident = format_ident!("{trait_ident}{name}");
            let ref_ident = format_ident!("{trait_ident}{name}Ref");

            let (named_fields, named_ref_fields): (_, Punctuated<_, _>) = args
                .filter_map(|n| match &n.1 {
                    FnArg::Typed(n) => Some(n),
                    _ => None,
                })
                .map(|n| {
                    (
                        Field {
                            attrs: vec![],
                            vis: vis.clone(),
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
                            vis: vis.clone(),
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
            RpcItem {
                name,
                index,
                msg_struct: ItemStruct {
                    attrs: parse_quote!(#[derive(serde::Serialize, serde::Deserialize,Debug)]),
                    vis: vis.clone(),
                    struct_token: Default::default(),
                    ident,
                    generics: Default::default(),
                    fields: Fields::Named(FieldsNamed {
                        brace_token: Default::default(),
                        named: named_fields,
                    }),
                    semi_token: None,
                },
                msg_ref_struct: ItemStruct {
                    attrs: parse_quote!(#[derive(serde::Serialize,Debug)]),
                    vis: vis.clone(),
                    struct_token: Default::default(),
                    ident: ref_ident.clone(),
                    generics: (!named_ref_fields.is_empty())
                        .then_some(Generics {
                            lt_token: Some(Default::default()),
                            params: once(GenericParam::Lifetime(LifetimeParam {
                                attrs: vec![],
                                lifetime: ref_lifetime.clone(),
                                colon_token: None,
                                bounds: Default::default(),
                            }))
                            .collect(),
                            gt_token: Some(Default::default()),
                            where_clause: None,
                        })
                        .unwrap_or_default(),
                    fields: Fields::Named(FieldsNamed {
                        brace_token: Default::default(),
                        named: named_ref_fields,
                    }),
                    semi_token: None,
                },
                // reply_type: {
                //     let ty = match &n.sig.output {
                //         ReturnType::Default => parse_quote!(()),
                //         ReturnType::Type(_, ty) => *ty.clone(),
                //     };
                //     get_future_output(no_async, &ty)
                // },
                // stream_item_type: found_input_trans_stream
                //     .map(|(_, _, item_ty, _)| item_ty.clone()),
            }
        })
        .collect();

    let variant_def = {
        let names = rpc_items.iter().map(|n| format_ident!("{}", &n.name));
        let idents = rpc_items.iter().map(|n| format_ident!("T{}", n.index));
        let idents2 = rpc_items.iter().map(|n| format_ident!("T{}", n.index));
        quote! {
           #[xy_rpc::enum_derive(Future)]
           enum #variant_ident<#(#idents),*> {
              #(#names(#idents2)),*
           }
        }
    };
    let mut wasm_extra = vec![];

    let (rpc_call_fn, rpc_call_fn_impl, handle_impl,wasm_rpc_call_proxy): (Vec<_>, Vec<_>, Vec<_>, Vec<_>) = ast
      .items
      .iter()
      .filter_map(|n| match n {
         syn::TraitItem::Fn(n) => Some(n),
         _ => return None,
      })
      .enumerate()
      .map(|(i, trait_item_fn)| {
         let id = (i + 1) as u16;
         let stream_arg = get_input_trans_stream(trait_item_fn);
         let output_stream_item = get_output_stream_item(&trait_item_fn);
         let fields: Punctuated<Ident, Token![,]> = trait_item_fn
            .sig
            .inputs
            .iter()
            .enumerate()
            .filter(|n| !stream_arg.is_some_and(|a| a.3 == n.0))
            .filter_map(|(_, n)| match n {
               FnArg::Typed(n) => Some(match n.pat.as_ref() {
                  syn::Pat::Ident(n) => n.ident.clone(),
                  _ => panic!("only support named fields"),
               }),
               _ => None,
            })
            .collect();
         let mut rpc_call_fn = trait_item_fn.sig.clone();
         let mut wasm_rpc_call_fn = trait_item_fn.sig.clone();
         for (i,n) in wasm_rpc_call_fn.inputs.iter_mut().enumerate() {
            let pat_ty = match n {
               FnArg::Typed(n) => n,
               _ => continue,
            };
            let Type::Path(ty) = &*pat_ty.ty else {
               continue
            };
            let Some(segment) = ty.path.segments.last() else {
               continue
            };
            if segment.ident == "TransStream" {
                let PathArguments::AngleBracketed(args) = &segment.arguments else {
                    unreachable!("Stream Type invalid. {:?}", segment.arguments)
                };
                let GenericArgument::Type(item_type) = args.args.iter().next().unwrap() else {
                    unreachable!("Stream Item Type invalid. {:?}", args.args)
                };
                let param_type_ident = format_ident!("{}{}{i}ParamTy",ast.ident,trait_item_fn.sig.ident.to_string().to_case(Case::UpperCamel));
                wasm_extra.push(quote! {
                    #[cfg(target_arch = "wasm32")]
                    #[wasm_bindgen::prelude::wasm_bindgen(getter_with_clone)]
                    pub struct #param_type_ident(pub #item_type);
                });
                let param_type = format!("AsyncIterator<{}[0]>", param_type_ident.to_string());
               pat_ty.ty = parse_quote!(js_sys::AsyncIterator);
               pat_ty.attrs.push(parse_quote!{
                   #[wasm_bindgen::prelude::wasm_bindgen(unchecked_param_type = #param_type)]
               });
            }
         }
         for (i,arg) in rpc_call_fn.inputs.iter_mut().enumerate() {
            if let FnArg::Typed(arg) = arg {
               if stream_arg.is_some_and(|n| n.3 == i) {
                  let item_ty = stream_arg.as_ref().unwrap().2;
                  arg.ty = parse_quote!{
                     impl Stream<Item = Result<#item_ty, RpcError>> + xy_rpc::maybe_send::MaybeSend + 'static
                  };
               }else{
                  arg.ty = Box::new(Type::Reference(TypeReference {
                     and_token: Default::default(),
                     lifetime: Some(ref_lifetime.clone()),
                     mutability: None,
                     elem: Box::new(*arg.ty.clone()),
                  }));
               }
            };
         }
         if !rpc_call_fn.inputs.is_empty() {
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
         }
         let is_async = rpc_call_fn.asyncness.is_some();
         rpc_call_fn.output = match rpc_call_fn.output {
            ReturnType::Default => {
               if !is_async {
                  panic!("rpc fn return type must impl Future");
               }
               rpc_call_fn.asyncness = None;
               parse_quote! {
                  -> impl core::future::Future<Output = Result<(), xy_rpc::RpcError>> + xy_rpc::maybe_send::MaybeSend +'static
               }
            }
            ReturnType::Type(_, ty) => {
               let output_ty = get_future_output(!is_async, &*ty);
               if is_async {
                  rpc_call_fn.asyncness = None;
               }
               parse_quote! {
                  -> impl core::future::Future<Output = Result<#output_ty, xy_rpc::RpcError>> + xy_rpc::maybe_send::MaybeSend +'static
               }
            }
         };
          let mut wasm_attr = None;
         wasm_rpc_call_fn.output = match wasm_rpc_call_fn.output {
            ReturnType::Default => {
               if !is_async {
                  panic!("rpc fn return type must impl Future");
               }
               parse_quote! {
                  -> Result<(), wasm_bindgen::JsValue>
               }
            }
            ReturnType::Type(_, ty) => {
               let output_ty = get_future_output(!is_async, &*ty);
               if !is_async {
                  wasm_rpc_call_fn.asyncness = Some(Default::default());
               }

                if let Some(item_ty) = &output_stream_item {
                    let return_type_ident = format_ident!("{}{}{i}ReturnStreamItemTy",ast.ident,trait_item_fn.sig.ident.to_string().to_case(Case::UpperCamel));
                        wasm_extra.push(quote! {
                        #[cfg(target_arch = "wasm32")]
                        #[wasm_bindgen::prelude::wasm_bindgen(getter_with_clone)]
                        pub struct #return_type_ident(pub #item_ty);
                    });
                    let return_type = format!("AsyncIterable<{}[0]>", return_type_ident.to_string());
                    wasm_attr = Some(quote!{
                        #[wasm_bindgen::prelude::wasm_bindgen(unchecked_return_type = #return_type)]
                    });
                    parse_quote! {
                      ->  Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>
                   }
                }else {
                    parse_quote! {
                      ->  Result<#output_ty, wasm_bindgen::JsValue>
                   }
                }
            }
         };
         let item_ident = format_ident!("{}",trait_item_fn.sig.ident.to_string().to_case(Case::UpperCamel));
         let ident = format_ident!("{trait_ident}{item_ident}");
         let ref_ident = format_ident!("{trait_ident}{item_ident}Ref");
         let fn_ident = &rpc_call_fn.ident;
         // let item_name = fn_ident.to_string().to_case(Case::UpperCamel);
         // let enum_item_ident = format_ident!("{}", item_name);
         // output_stream_item
         let msg_stream = match &stream_arg {
            None => quote!{
               None::<xy_rpc::EmptyStream>
            },
            Some((pat,..)) => {
               let stream_arg = &pat.pat;
               quote! {
                  Some(#stream_arg)
               }
            }
         };
         let rcp_type = match (stream_arg, &output_stream_item) {
            (None,None) => quote!(xy_rpc::Unary),
            (None,Some(..)) => quote!(xy_rpc::ReplyStreaming),
            (Some(..),None) => quote!(xy_rpc::MsgStreaming),
            (Some(..),Some(..)) => quote!(xy_rpc::BidirectionalStreaming)
         };
         (
            quote! {
                #rpc_call_fn
            },
            quote! {
                #rpc_call_fn {
                    let future = self.call(&#ref_ident { #fields },#msg_stream,#id as _,#rcp_type);
                    async move {
                        let reply = future.await?;
                        Ok(reply)
                    }
                }
            },
            {
                  let reply_handle = match &output_stream_item {
                     None => quote! {
                            let reply = xy_rpc::temp_buf::with_buf(|buf| {
                                   serde_format.serialize_to_writer_optimized(buf, &reply)
                                       .map(|_| buf.split().freeze())
                                       .map_err(xy_rpc::RpcError::SerdeError)
                               })?;
                            Ok(xy_rpc::HandleReply::Once(reply))
                     },
                     Some(item_ty) => quote! {
                        use futures_util::stream::StreamExt;
                        Ok(xy_rpc::HandleReply::Stream(Box::pin(reply.map(move |n| {
                              let n = n?;
                               let reply = xy_rpc::temp_buf::with_buf(|buf| {
                                      serde_format.serialize_to_writer_optimized::<#item_ty>(buf, &n)
                                          .map(|_| buf.split().freeze())
                                          .map_err(xy_rpc::RpcError::SerdeError)
                                  })?;
                                Ok(reply)
                        }))))
                     }
                  };
               match stream_arg {
                  None => quote! {
                     #id => {
                           let serde_format = serde_format.clone();
                           let #ident { #fields } = serde_format.deserialize_from_slice_optimized(&msg.msg).map_err(xy_rpc::RpcError::SerdeError)?;
                           Ok(#variant_ident::#item_ident(async move {
                               let reply = self.service.#fn_ident(#fields).await;
                               #reply_handle
                           }))
                     }
                  },
                  Some((stream_arg, _, _, _)) => {
                     let stream_arg = &stream_arg.pat;
                     let d:Option<Comma> = (!fields.is_empty()).then_some(Comma::default());
                     quote! {
                        #id => {
                           let serde_format = serde_format.clone();
                           let #ident { #fields } = serde_format.deserialize_from_slice_optimized(&msg.msg).map_err(xy_rpc::RpcError::SerdeError)?;
                           Ok(#variant_ident::#item_ident(async move {
                               let reply = self.service.#fn_ident(#fields #d TransStream::new(
                                 #stream_arg.unwrap().into_stream(),
                                 serde_format.clone(),
                              )).await;
                               #reply_handle
                           }))
                        }
                     }
                  }
               }
            },
            {
                let r_handle = match &output_stream_item {
                    None => quote! {
                        r
                     },
                    Some(_item_ty) => quote! {
                        let r = r?;
                        xy_rpc::try_stream_to_js_async_iterator(r)
                     }
                };
               match stream_arg {
                  None => {
                     let fields = fields.iter();
                     quote! {
                        #wasm_attr
                        pub #wasm_rpc_call_fn {
                          let r = self.rpc_channel.#fn_ident(#(&#fields),*).await.map_err(|err| wasm_bindgen::JsValue::from(err.to_string()));
                          #r_handle
                        }
                     }
                  },
                  Some((stream_arg, _, item_ty, _)) => {
                     let stream_arg = &stream_arg.pat;
                     let d:Option<Comma> = (!fields.is_empty()).then_some(Comma::default());
                     let fields = fields.iter();
                     quote! {
                        #wasm_attr
                        pub #wasm_rpc_call_fn {
                          let js_stream = wasm_bindgen_futures::stream::JsStream::from(#stream_arg);
                          let r = self.rpc_channel.#fn_ident(#(&#fields),* #d futures_util::stream::unfold(js_stream,move |mut state| async move {
                                 let r = state.next().await?;
                                 match r {
                                     Ok(value) => {
                                         match value.into_serde::<#item_ty>().map_err(|err| xy_rpc::RpcError::SerdeError(Box::new(err))) {
                                            Ok(value) => Some((Ok(value),state)),
                                            Err(err) => Some((Err(err),state))
                                         }
                                     }
                                     Err(err) => {
                                         return Some((Err(xy_rpc::RpcError::OtherError {
                                           message: format!("{err:?}")
                                        }),state))
                                     }
                                 }
                             }
                           )).await.map_err(|err| wasm_bindgen::JsValue::from(err.to_string()));
                           #r_handle
                        }
                     }
                  }
               }
            },
         )
      })
      .collect();

    let schema = quote! {
        #[derive(Clone, Debug, Default)]
        #vis struct #schema_ident;
        impl xy_rpc::RpcSchema for #schema_ident
        {
        }
    };

    let impls = quote! {
        impl<T> xy_rpc::RpcMsgHandler<#schema_ident> for xy_rpc::RpcMsgHandlerWrapper<T>
        where
            T: #trait_ident,
        {
            fn handle(&self, msg: xy_rpc::RpcRawMsg, stream: Option<xy_rpc::flume::Receiver<xy_rpc::bytes::Bytes>>, serde_format: &impl xy_rpc::formats::SerdeFormat) -> Result<impl core::future::Future<Output=Result<xy_rpc::HandleReply,xy_rpc::RpcError>> + xy_rpc::maybe_send::MaybeSend,xy_rpc::RpcError> {
               use xy_rpc::bytes::BufMut;
                 match msg.msg_kind {
                     #(#handle_impl)*
                     _ => Err(xy_rpc::RpcError::InvalidMsgKind)
                 }
            }
        }
        #vis trait #caller_ident {
            #(#rpc_call_fn;)*
        }
        impl<CF> #caller_ident for xy_rpc::XyRpcChannel<CF,#schema_ident> where CF: xy_rpc::formats::SerdeFormat {
            #(#rpc_call_fn_impl)*
        }
    };

    let msg_structs = rpc_items.iter().map(|n| &n.msg_struct);
    let msg_ref_structs = rpc_items.iter().map(|n| &n.msg_ref_struct);

    let wasm_export = {
        // let mod_ident = format_ident!("{}", ast.ident.to_string().to_case(Case::Snake));
        let channel_fn = format_ident!("{}", ast.ident.to_string().to_case(Case::Snake));
        // let schema_ident = format_ident!("{}Schema", ast.ident);
        quote! {
           #[cfg(target_arch = "wasm32")]
           #[wasm_bindgen::prelude::wasm_bindgen]
           pub async fn #channel_fn(url: &str) -> Result<#wasm_channel_proxy, wasm_bindgen::JsValue> {
              use xy_rpc::ChannelBuilderWebExt;
               let (read, write) = xy_rpc::get_http_stream_from_url(url).await?;
               let (rpc_channel, future) = ChannelBuilder::new(xy_rpc::formats::JsonFormat::default())
                   .only_call::<#schema_ident>()
                   .build_from_web_stream(read, write);
               wasm_bindgen_futures::spawn_local(async move {
                   let r = future.await;
               });
               Ok(#wasm_channel_proxy { rpc_channel })
           }

           #[cfg(target_arch = "wasm32")]
           #[wasm_bindgen::prelude::wasm_bindgen]
           pub struct #wasm_channel_proxy {
               rpc_channel: xy_rpc::XyRpcChannel<xy_rpc::formats::JsonFormat, #schema_ident>,
           }

           #[cfg(target_arch = "wasm32")]
           #[wasm_bindgen::prelude::wasm_bindgen]
           impl #wasm_channel_proxy {
              #(#wasm_rpc_call_proxy)*
           }
        }
    };

    quote! {
        #ast
        #schema
        #variant_def
        #impls
        #(#msg_structs)*
        #(#msg_ref_structs)*
        #(#wasm_extra)*
        #wasm_export
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

fn get_output_stream_item(n: &TraitItemFn) -> Option<Type> {
    match &n.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => {
            let output = get_future_output(n.sig.asyncness.is_none(), ty.as_ref());
            if let Type::ImplTrait(impl_trait) = &output {
                impl_trait.bounds.iter().find_map(|n| match n {
                    TypeParamBound::Trait(t) => {
                        let x = t.path.segments.iter().find(|n| n.ident == "Stream");
                        if let Some(x) = x {
                            let PathArguments::AngleBracketed(args) = &x.arguments else {
                                panic!("invalid return type")
                            };
                            args.args.iter().find_map(|n| match n {
                                GenericArgument::AssocType(a) => {
                                    if a.ident == "Item" {
                                        let ty = get_result_ok(&a.ty);
                                        Some(ty.clone())
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
                })
            } else {
                None
            }
        }
    }
}

fn get_input_trans_stream(n: &TraitItemFn) -> Option<(&PatType, &PathSegment, &Type, usize)> {
    n.sig.inputs.iter().enumerate().find_map(|(i, n)| {
        let pat_ty = match n {
            FnArg::Typed(n) => n,
            _ => return None,
        };
        let Type::Path(ty) = &*pat_ty.ty else {
            return None;
        };
        let segment = ty.path.segments.last()?;
        (segment.ident == "TransStream").then(|| {
            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                unreachable!("Stream Type invalid. {:?}", segment.arguments)
            };
            let GenericArgument::Type(item_type) = args.args.iter().next().unwrap() else {
                unreachable!("Stream Item Type invalid. {:?}", args.args)
            };
            (pat_ty, segment, item_type, i)
        })
    })
}

fn get_result_ok(ty: &Type) -> &Type {
    let Type::Path(ty) = ty else {
        panic!("invalid result type")
    };
    let segment = &ty.path.segments.last().unwrap();
    if segment.ident != "Result" {
        panic!("stream item must is Result<T,RpcError>")
    }
    let arguments = &segment.arguments;
    let PathArguments::AngleBracketed(n) = arguments else {
        panic!("invalid result type")
    };
    let GenericArgument::Type(ty) = n.args.first().expect("invalid result type") else {
        panic!("invalid result type")
    };
    ty
}
