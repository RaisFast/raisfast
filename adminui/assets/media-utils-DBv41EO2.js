import{a as p}from"./types-C-xO67t8.js";import{b5 as o,a7 as l,Z as d}from"./index-DwmPvwsm.js";import{A as f}from"./archive-BMOtwSqs.js";import{T as u}from"./type-Bh0Chp7E.js";import{F as h}from"./file-BqZzM3iA.js";/**
 * @license lucide-react v1.17.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const x=[["path",{d:"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",key:"1oefj6"}],["path",{d:"M14 2v5a1 1 0 0 0 1 1h5",key:"wfsgrz"}],["path",{d:"M10 12.5 8 15l2 2.5",key:"1tg20x"}],["path",{d:"m14 12.5 2 2.5-2 2.5",key:"yinavb"}]],g=o("file-code",x);/**
 * @license lucide-react v1.17.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const y=[["path",{d:"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",key:"1oefj6"}],["path",{d:"M14 2v5a1 1 0 0 0 1 1h5",key:"wfsgrz"}],["path",{d:"M11 18h2",key:"12mj7e"}],["path",{d:"M12 12v6",key:"3ahymv"}],["path",{d:"M9 13v-.5a.5.5 0 0 1 .5-.5h5a.5.5 0 0 1 .5.5v.5",key:"qbrxap"}]],k=o("file-type",y);/**
 * @license lucide-react v1.17.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const v=[["path",{d:"M9 18V5l12-2v13",key:"1jmyc2"}],["circle",{cx:"6",cy:"18",r:"3",key:"fqmcym"}],["circle",{cx:"18",cy:"16",r:"3",key:"1hluhg"}]],M=o("music",v);/**
 * @license lucide-react v1.17.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const b=[["path",{d:"M9 3H5a2 2 0 0 0-2 2v4m6-6h10a2 2 0 0 1 2 2v4M9 3v18m0 0h10a2 2 0 0 0 2-2V9M9 21H5a2 2 0 0 1-2-2V9m0 0h18",key:"gugj83"}]],w=o("table-2",b);/**
 * @license lucide-react v1.17.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const F=[["path",{d:"m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5",key:"ftymec"}],["rect",{x:"2",y:"6",width:"14",height:"12",rx:"2",key:"158x01"}]],_=o("video",F),m=[{key:"image",label:"Images",icon:l,mimes:["image/jpeg","image/png","image/gif","image/webp","image/svg+xml","image/bmp","image/avif","image/tiff","image/x-icon","image/heic","image/heif"]},{key:"video",label:"Video",icon:_,mimes:["video/mp4","video/webm","video/quicktime","video/x-matroska"]},{key:"audio",label:"Audio",icon:M,mimes:["audio/mpeg","audio/ogg","audio/wav","audio/aac","audio/flac","audio/opus","audio/mp4"]},{key:"document",label:"Docs",icon:d,mimes:["application/pdf","application/msword","application/vnd.openxmlformats-officedocument.wordprocessingml.document","application/vnd.ms-powerpoint","application/vnd.openxmlformats-officedocument.presentationml.presentation","application/epub+zip","application/rtf"]},{key:"spreadsheet",label:"Sheets",icon:w,mimes:["application/vnd.ms-excel","application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"]},{key:"archive",label:"Archives",icon:f,mimes:["application/zip","application/x-tar","application/gzip","application/x-rar-compressed","application/x-7z-compressed"]},{key:"text",label:"Text",icon:k,mimes:["text/plain","text/csv","text/markdown","application/json","text/html","application/xml","text/yaml","text/x-ini","application/toml","text/css"]},{key:"scripts",label:"Scripts",icon:g,mimes:["application/javascript","text/x-shellscript","application/x-httpd-php","text/x-python","application/sql"]},{key:"font",label:"Fonts",icon:u,mimes:["font/ttf","font/otf","font/woff","font/woff2"]}];function r(e){for(const i of m)if(i.mimes.includes(e))return i.key;return"other"}function C(e){const i=r(e),t=m.find(s=>s.key===i);return(t==null?void 0:t.icon)??h}function I(e,i){return i==="all"?!0:r(e.mimetype)===i}function E(e){if(e==="all"||e==="other")return"";const i=m.find(t=>t.key===e);return(i==null?void 0:i.mimes.join(","))??""}const z=p;function N(e){return z[e]??e}function q(e){return e<1024?`${e} B`:e<1024*1024?`${(e/1024).toFixed(1)} KB`:e<1024*1024*1024?`${(e/(1024*1024)).toFixed(1)} MB`:`${(e/(1024*1024*1024)).toFixed(1)} GB`}function B(e){return e.startsWith("image/")}function L(e){return e.startsWith("video/")}function S(e){return e.startsWith("audio/")}function D(e){return e==="application/pdf"}function W(e,i,t){return[...e].sort((n,c)=>{let a;switch(i){case"filename":a=n.filename.localeCompare(c.filename);break;case"size":a=n.size-c.size;break;case"created_at":default:a=new Date(n.created_at).getTime()-new Date(c.created_at).getTime();break}return t==="desc"?-a:a})}export{m as F,w as T,_ as V,g as a,r as b,C as c,B as d,D as e,q as f,E as g,L as h,S as i,N as j,I as m,W as s};
