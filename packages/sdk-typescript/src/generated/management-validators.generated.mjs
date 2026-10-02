/*
 * Generated from Flaggo v3 JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */
"use strict";
export const validateDecisionContract = validate21;
const schema32 = {"type":"object","additionalProperties":false,"required":["name","expression_syntax","attributes","result"],"properties":{"name":{"$ref":"#/$defs/DecisionName"},"expression_syntax":{"const":"flaggo.cel/v1"},"attributes":{"type":"array","maxItems":128,"items":{"$ref":"#/$defs/Attribute"},"description":"User-declared runtime attributes. Attribute names must be unique; order is non-semantic."},"result":{"$ref":"#/$defs/Result"},"authoredExecutable":{"$ref":"#/$defs/AuthoredExecutable"},"learning":{"$ref":"#/$defs/Learning"}}};
const schema33 = {"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9._-]*$"};
const func1 = require("ajv/dist/runtime/ucs2length").default;
const pattern4 = new RegExp("^[A-Za-z][A-Za-z0-9._-]*$", "u");
const schema34 = {"type":"object","additionalProperties":false,"required":["name","schema"],"properties":{"name":{"$ref":"#/$defs/AttributeName"},"schema":{"$ref":"#/$defs/ValueSchema"}}};
const schema35 = {"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9_]*$","description":"A user-declared attribute name. Names beginning with '_' are reserved for Flaggo internal attributes."};
const pattern5 = new RegExp("^[A-Za-z][A-Za-z0-9_]*$", "u");
const schema36 = {"oneOf":[{"$ref":"#/$defs/NullValueSchema"},{"$ref":"#/$defs/BooleanValueSchema"},{"$ref":"#/$defs/NumberValueSchema"},{"$ref":"#/$defs/IntegerValueSchema"},{"$ref":"#/$defs/StringValueSchema"},{"$ref":"#/$defs/ArrayValueSchema"},{"$ref":"#/$defs/ObjectValueSchema"}],"description":"A JSON Schema 2020-12 document in the bounded flaggo.value-schema/v1 profile."};
const schema37 = {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"null"},"description":{"type":"string"}}};
const schema38 = {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"boolean"},"description":{"type":"string"}}};
const schema39 = {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"number"},"minimum":{"type":"number"},"maximum":{"type":"number"},"exclusiveMinimum":{"type":"number"},"exclusiveMaximum":{"type":"number"},"multipleOf":{"type":"number","exclusiveMinimum":0},"description":{"type":"string"}}};
const schema40 = {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"integer"},"minimum":{"type":"integer"},"maximum":{"type":"integer"},"exclusiveMinimum":{"type":"integer"},"exclusiveMaximum":{"type":"integer"},"multipleOf":{"type":"integer","minimum":1},"description":{"type":"string"}}};
const schema41 = {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"string"},"minLength":{"type":"integer","minimum":0,"maximum":16384},"maxLength":{"type":"integer","minimum":0,"maximum":16384},"description":{"type":"string"}}};
const schema42 = {"type":"object","additionalProperties":false,"required":["type","items"],"properties":{"type":{"const":"array"},"items":{"$ref":"#/$defs/ValueSchema"},"minItems":{"type":"integer","minimum":0,"maximum":256},"maxItems":{"type":"integer","minimum":0,"maximum":256},"uniqueItems":{"type":"boolean"},"description":{"type":"string"}}};
const wrapper0 = {validate: validate23};

function validate24(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate24.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.items === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "items"},message:"must have required property '"+"items"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((((((key0 === "type") || (key0 === "items")) || (key0 === "minItems")) || (key0 === "maxItems")) || (key0 === "uniqueItems")) || (key0 === "description"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.type !== undefined){
if("array" !== data.type){
const err3 = {instancePath:instancePath+"/type",schemaPath:"#/properties/type/const",keyword:"const",params:{allowedValue: "array"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.items !== undefined){
if(!(wrapper0.validate(data.items, {instancePath:instancePath+"/items",parentData:data,parentDataProperty:"items",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper0.validate.errors : vErrors.concat(wrapper0.validate.errors);
errors = vErrors.length;
}
}
if(data.minItems !== undefined){
let data2 = data.minItems;
if(!(((typeof data2 == "number") && (!(data2 % 1) && !isNaN(data2))) && (isFinite(data2)))){
const err4 = {instancePath:instancePath+"/minItems",schemaPath:"#/properties/minItems/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if((typeof data2 == "number") && (isFinite(data2))){
if(data2 > 256 || isNaN(data2)){
const err5 = {instancePath:instancePath+"/minItems",schemaPath:"#/properties/minItems/maximum",keyword:"maximum",params:{comparison: "<=", limit: 256},message:"must be <= 256"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data2 < 0 || isNaN(data2)){
const err6 = {instancePath:instancePath+"/minItems",schemaPath:"#/properties/minItems/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
}
if(data.maxItems !== undefined){
let data3 = data.maxItems;
if(!(((typeof data3 == "number") && (!(data3 % 1) && !isNaN(data3))) && (isFinite(data3)))){
const err7 = {instancePath:instancePath+"/maxItems",schemaPath:"#/properties/maxItems/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if((typeof data3 == "number") && (isFinite(data3))){
if(data3 > 256 || isNaN(data3)){
const err8 = {instancePath:instancePath+"/maxItems",schemaPath:"#/properties/maxItems/maximum",keyword:"maximum",params:{comparison: "<=", limit: 256},message:"must be <= 256"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(data3 < 0 || isNaN(data3)){
const err9 = {instancePath:instancePath+"/maxItems",schemaPath:"#/properties/maxItems/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
}
if(data.uniqueItems !== undefined){
if(typeof data.uniqueItems !== "boolean"){
const err10 = {instancePath:instancePath+"/uniqueItems",schemaPath:"#/properties/uniqueItems/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err11 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
}
else {
const err12 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
validate24.errors = vErrors;
return errors === 0;
}
validate24.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema43 = {"type":"object","additionalProperties":false,"required":["type","properties","additionalProperties"],"properties":{"type":{"const":"object"},"properties":{"type":"object","maxProperties":256,"additionalProperties":{"$ref":"#/$defs/ValueSchema"}},"required":{"type":"array","maxItems":256,"items":{"type":"string"},"uniqueItems":true,"description":"Required property names form an unordered set; order is non-semantic."},"additionalProperties":{"const":false},"minProperties":{"type":"integer","minimum":0,"maximum":256},"maxProperties":{"type":"integer","minimum":0,"maximum":256},"description":{"type":"string"}}};

function validate26(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate26.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.properties === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "properties"},message:"must have required property '"+"properties"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.additionalProperties === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "additionalProperties"},message:"must have required property '"+"additionalProperties"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((((((key0 === "type") || (key0 === "properties")) || (key0 === "required")) || (key0 === "additionalProperties")) || (key0 === "minProperties")) || (key0 === "maxProperties")) || (key0 === "description"))){
const err3 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.type !== undefined){
if("object" !== data.type){
const err4 = {instancePath:instancePath+"/type",schemaPath:"#/properties/type/const",keyword:"const",params:{allowedValue: "object"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.properties !== undefined){
let data1 = data.properties;
if(data1 && typeof data1 == "object" && !Array.isArray(data1)){
if(Object.keys(data1).length > 256){
const err5 = {instancePath:instancePath+"/properties",schemaPath:"#/properties/properties/maxProperties",keyword:"maxProperties",params:{limit: 256},message:"must NOT have more than 256 properties"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
for(const key1 in data1){
if(!(wrapper0.validate(data1[key1], {instancePath:instancePath+"/properties/" + key1.replace(/~/g, "~0").replace(/\//g, "~1"),parentData:data1,parentDataProperty:key1,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper0.validate.errors : vErrors.concat(wrapper0.validate.errors);
errors = vErrors.length;
}
}
}
else {
const err6 = {instancePath:instancePath+"/properties",schemaPath:"#/properties/properties/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.required !== undefined){
let data3 = data.required;
if(Array.isArray(data3)){
if(data3.length > 256){
const err7 = {instancePath:instancePath+"/required",schemaPath:"#/properties/required/maxItems",keyword:"maxItems",params:{limit: 256},message:"must NOT have more than 256 items"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
const len0 = data3.length;
for(let i0=0; i0<len0; i0++){
if(typeof data3[i0] !== "string"){
const err8 = {instancePath:instancePath+"/required/" + i0,schemaPath:"#/properties/required/items/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
let i1 = data3.length;
let j0;
if(i1 > 1){
const indices0 = {};
for(;i1--;){
let item0 = data3[i1];
if(typeof item0 !== "string"){
continue;
}
if(typeof indices0[item0] == "number"){
j0 = indices0[item0];
const err9 = {instancePath:instancePath+"/required",schemaPath:"#/properties/required/uniqueItems",keyword:"uniqueItems",params:{i: i1, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i1+" are identical)"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
break;
}
indices0[item0] = i1;
}
}
}
else {
const err10 = {instancePath:instancePath+"/required",schemaPath:"#/properties/required/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.additionalProperties !== undefined){
if(false !== data.additionalProperties){
const err11 = {instancePath:instancePath+"/additionalProperties",schemaPath:"#/properties/additionalProperties/const",keyword:"const",params:{allowedValue: false},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.minProperties !== undefined){
let data6 = data.minProperties;
if(!(((typeof data6 == "number") && (!(data6 % 1) && !isNaN(data6))) && (isFinite(data6)))){
const err12 = {instancePath:instancePath+"/minProperties",schemaPath:"#/properties/minProperties/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if((typeof data6 == "number") && (isFinite(data6))){
if(data6 > 256 || isNaN(data6)){
const err13 = {instancePath:instancePath+"/minProperties",schemaPath:"#/properties/minProperties/maximum",keyword:"maximum",params:{comparison: "<=", limit: 256},message:"must be <= 256"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(data6 < 0 || isNaN(data6)){
const err14 = {instancePath:instancePath+"/minProperties",schemaPath:"#/properties/minProperties/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
}
if(data.maxProperties !== undefined){
let data7 = data.maxProperties;
if(!(((typeof data7 == "number") && (!(data7 % 1) && !isNaN(data7))) && (isFinite(data7)))){
const err15 = {instancePath:instancePath+"/maxProperties",schemaPath:"#/properties/maxProperties/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
if((typeof data7 == "number") && (isFinite(data7))){
if(data7 > 256 || isNaN(data7)){
const err16 = {instancePath:instancePath+"/maxProperties",schemaPath:"#/properties/maxProperties/maximum",keyword:"maximum",params:{comparison: "<=", limit: 256},message:"must be <= 256"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
if(data7 < 0 || isNaN(data7)){
const err17 = {instancePath:instancePath+"/maxProperties",schemaPath:"#/properties/maxProperties/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err18 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
}
}
else {
const err19 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
validate26.errors = vErrors;
return errors === 0;
}
validate26.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate23(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate23.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err0 = {instancePath,schemaPath:"#/$defs/NullValueSchema/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "type") || (key0 === "description"))){
const err1 = {instancePath,schemaPath:"#/$defs/NullValueSchema/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.type !== undefined){
if("null" !== data.type){
const err2 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/NullValueSchema/properties/type/const",keyword:"const",params:{allowedValue: "null"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err3 = {instancePath:instancePath+"/description",schemaPath:"#/$defs/NullValueSchema/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/$defs/NullValueSchema/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs8 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err5 = {instancePath,schemaPath:"#/$defs/BooleanValueSchema/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
for(const key1 in data){
if(!((key1 === "type") || (key1 === "description"))){
const err6 = {instancePath,schemaPath:"#/$defs/BooleanValueSchema/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.type !== undefined){
if("boolean" !== data.type){
const err7 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/BooleanValueSchema/properties/type/const",keyword:"const",params:{allowedValue: "boolean"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err8 = {instancePath:instancePath+"/description",schemaPath:"#/$defs/BooleanValueSchema/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
}
else {
const err9 = {instancePath,schemaPath:"#/$defs/BooleanValueSchema/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
var _valid0 = _errs8 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
const _errs15 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err10 = {instancePath,schemaPath:"#/$defs/NumberValueSchema/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
for(const key2 in data){
if(!(((((((key2 === "type") || (key2 === "minimum")) || (key2 === "maximum")) || (key2 === "exclusiveMinimum")) || (key2 === "exclusiveMaximum")) || (key2 === "multipleOf")) || (key2 === "description"))){
const err11 = {instancePath,schemaPath:"#/$defs/NumberValueSchema/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key2},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.type !== undefined){
if("number" !== data.type){
const err12 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/NumberValueSchema/properties/type/const",keyword:"const",params:{allowedValue: "number"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data.minimum !== undefined){
let data5 = data.minimum;
if(!((typeof data5 == "number") && (isFinite(data5)))){
const err13 = {instancePath:instancePath+"/minimum",schemaPath:"#/$defs/NumberValueSchema/properties/minimum/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.maximum !== undefined){
let data6 = data.maximum;
if(!((typeof data6 == "number") && (isFinite(data6)))){
const err14 = {instancePath:instancePath+"/maximum",schemaPath:"#/$defs/NumberValueSchema/properties/maximum/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data.exclusiveMinimum !== undefined){
let data7 = data.exclusiveMinimum;
if(!((typeof data7 == "number") && (isFinite(data7)))){
const err15 = {instancePath:instancePath+"/exclusiveMinimum",schemaPath:"#/$defs/NumberValueSchema/properties/exclusiveMinimum/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.exclusiveMaximum !== undefined){
let data8 = data.exclusiveMaximum;
if(!((typeof data8 == "number") && (isFinite(data8)))){
const err16 = {instancePath:instancePath+"/exclusiveMaximum",schemaPath:"#/$defs/NumberValueSchema/properties/exclusiveMaximum/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
if(data.multipleOf !== undefined){
let data9 = data.multipleOf;
if((typeof data9 == "number") && (isFinite(data9))){
if(data9 <= 0 || isNaN(data9)){
const err17 = {instancePath:instancePath+"/multipleOf",schemaPath:"#/$defs/NumberValueSchema/properties/multipleOf/exclusiveMinimum",keyword:"exclusiveMinimum",params:{comparison: ">", limit: 0},message:"must be > 0"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
else {
const err18 = {instancePath:instancePath+"/multipleOf",schemaPath:"#/$defs/NumberValueSchema/properties/multipleOf/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err19 = {instancePath:instancePath+"/description",schemaPath:"#/$defs/NumberValueSchema/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
}
}
else {
const err20 = {instancePath,schemaPath:"#/$defs/NumberValueSchema/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
var _valid0 = _errs15 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 2];
}
else {
if(_valid0){
valid0 = true;
passing0 = 2;
if(props0 !== true){
props0 = true;
}
}
const _errs32 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err21 = {instancePath,schemaPath:"#/$defs/IntegerValueSchema/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
for(const key3 in data){
if(!(((((((key3 === "type") || (key3 === "minimum")) || (key3 === "maximum")) || (key3 === "exclusiveMinimum")) || (key3 === "exclusiveMaximum")) || (key3 === "multipleOf")) || (key3 === "description"))){
const err22 = {instancePath,schemaPath:"#/$defs/IntegerValueSchema/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key3},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err22];
}
else {
vErrors.push(err22);
}
errors++;
}
}
if(data.type !== undefined){
if("integer" !== data.type){
const err23 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/IntegerValueSchema/properties/type/const",keyword:"const",params:{allowedValue: "integer"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err23];
}
else {
vErrors.push(err23);
}
errors++;
}
}
if(data.minimum !== undefined){
let data12 = data.minimum;
if(!(((typeof data12 == "number") && (!(data12 % 1) && !isNaN(data12))) && (isFinite(data12)))){
const err24 = {instancePath:instancePath+"/minimum",schemaPath:"#/$defs/IntegerValueSchema/properties/minimum/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err24];
}
else {
vErrors.push(err24);
}
errors++;
}
}
if(data.maximum !== undefined){
let data13 = data.maximum;
if(!(((typeof data13 == "number") && (!(data13 % 1) && !isNaN(data13))) && (isFinite(data13)))){
const err25 = {instancePath:instancePath+"/maximum",schemaPath:"#/$defs/IntegerValueSchema/properties/maximum/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err25];
}
else {
vErrors.push(err25);
}
errors++;
}
}
if(data.exclusiveMinimum !== undefined){
let data14 = data.exclusiveMinimum;
if(!(((typeof data14 == "number") && (!(data14 % 1) && !isNaN(data14))) && (isFinite(data14)))){
const err26 = {instancePath:instancePath+"/exclusiveMinimum",schemaPath:"#/$defs/IntegerValueSchema/properties/exclusiveMinimum/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err26];
}
else {
vErrors.push(err26);
}
errors++;
}
}
if(data.exclusiveMaximum !== undefined){
let data15 = data.exclusiveMaximum;
if(!(((typeof data15 == "number") && (!(data15 % 1) && !isNaN(data15))) && (isFinite(data15)))){
const err27 = {instancePath:instancePath+"/exclusiveMaximum",schemaPath:"#/$defs/IntegerValueSchema/properties/exclusiveMaximum/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err27];
}
else {
vErrors.push(err27);
}
errors++;
}
}
if(data.multipleOf !== undefined){
let data16 = data.multipleOf;
if(!(((typeof data16 == "number") && (!(data16 % 1) && !isNaN(data16))) && (isFinite(data16)))){
const err28 = {instancePath:instancePath+"/multipleOf",schemaPath:"#/$defs/IntegerValueSchema/properties/multipleOf/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err28];
}
else {
vErrors.push(err28);
}
errors++;
}
if((typeof data16 == "number") && (isFinite(data16))){
if(data16 < 1 || isNaN(data16)){
const err29 = {instancePath:instancePath+"/multipleOf",schemaPath:"#/$defs/IntegerValueSchema/properties/multipleOf/minimum",keyword:"minimum",params:{comparison: ">=", limit: 1},message:"must be >= 1"};
if(vErrors === null){
vErrors = [err29];
}
else {
vErrors.push(err29);
}
errors++;
}
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err30 = {instancePath:instancePath+"/description",schemaPath:"#/$defs/IntegerValueSchema/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err30];
}
else {
vErrors.push(err30);
}
errors++;
}
}
}
else {
const err31 = {instancePath,schemaPath:"#/$defs/IntegerValueSchema/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err31];
}
else {
vErrors.push(err31);
}
errors++;
}
var _valid0 = _errs32 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 3];
}
else {
if(_valid0){
valid0 = true;
passing0 = 3;
if(props0 !== true){
props0 = true;
}
}
const _errs49 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err32 = {instancePath,schemaPath:"#/$defs/StringValueSchema/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err32];
}
else {
vErrors.push(err32);
}
errors++;
}
for(const key4 in data){
if(!((((key4 === "type") || (key4 === "minLength")) || (key4 === "maxLength")) || (key4 === "description"))){
const err33 = {instancePath,schemaPath:"#/$defs/StringValueSchema/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key4},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err33];
}
else {
vErrors.push(err33);
}
errors++;
}
}
if(data.type !== undefined){
if("string" !== data.type){
const err34 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/StringValueSchema/properties/type/const",keyword:"const",params:{allowedValue: "string"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err34];
}
else {
vErrors.push(err34);
}
errors++;
}
}
if(data.minLength !== undefined){
let data19 = data.minLength;
if(!(((typeof data19 == "number") && (!(data19 % 1) && !isNaN(data19))) && (isFinite(data19)))){
const err35 = {instancePath:instancePath+"/minLength",schemaPath:"#/$defs/StringValueSchema/properties/minLength/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err35];
}
else {
vErrors.push(err35);
}
errors++;
}
if((typeof data19 == "number") && (isFinite(data19))){
if(data19 > 16384 || isNaN(data19)){
const err36 = {instancePath:instancePath+"/minLength",schemaPath:"#/$defs/StringValueSchema/properties/minLength/maximum",keyword:"maximum",params:{comparison: "<=", limit: 16384},message:"must be <= 16384"};
if(vErrors === null){
vErrors = [err36];
}
else {
vErrors.push(err36);
}
errors++;
}
if(data19 < 0 || isNaN(data19)){
const err37 = {instancePath:instancePath+"/minLength",schemaPath:"#/$defs/StringValueSchema/properties/minLength/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err37];
}
else {
vErrors.push(err37);
}
errors++;
}
}
}
if(data.maxLength !== undefined){
let data20 = data.maxLength;
if(!(((typeof data20 == "number") && (!(data20 % 1) && !isNaN(data20))) && (isFinite(data20)))){
const err38 = {instancePath:instancePath+"/maxLength",schemaPath:"#/$defs/StringValueSchema/properties/maxLength/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err38];
}
else {
vErrors.push(err38);
}
errors++;
}
if((typeof data20 == "number") && (isFinite(data20))){
if(data20 > 16384 || isNaN(data20)){
const err39 = {instancePath:instancePath+"/maxLength",schemaPath:"#/$defs/StringValueSchema/properties/maxLength/maximum",keyword:"maximum",params:{comparison: "<=", limit: 16384},message:"must be <= 16384"};
if(vErrors === null){
vErrors = [err39];
}
else {
vErrors.push(err39);
}
errors++;
}
if(data20 < 0 || isNaN(data20)){
const err40 = {instancePath:instancePath+"/maxLength",schemaPath:"#/$defs/StringValueSchema/properties/maxLength/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err40];
}
else {
vErrors.push(err40);
}
errors++;
}
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err41 = {instancePath:instancePath+"/description",schemaPath:"#/$defs/StringValueSchema/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err41];
}
else {
vErrors.push(err41);
}
errors++;
}
}
}
else {
const err42 = {instancePath,schemaPath:"#/$defs/StringValueSchema/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err42];
}
else {
vErrors.push(err42);
}
errors++;
}
var _valid0 = _errs49 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 4];
}
else {
if(_valid0){
valid0 = true;
passing0 = 4;
if(props0 !== true){
props0 = true;
}
}
const _errs60 = errors;
if(!(validate24(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate24.errors : vErrors.concat(validate24.errors);
errors = vErrors.length;
}
var _valid0 = _errs60 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 5];
}
else {
if(_valid0){
valid0 = true;
passing0 = 5;
if(props0 !== true){
props0 = true;
}
}
const _errs61 = errors;
if(!(validate26(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate26.errors : vErrors.concat(validate26.errors);
errors = vErrors.length;
}
var _valid0 = _errs61 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 6];
}
else {
if(_valid0){
valid0 = true;
passing0 = 6;
if(props0 !== true){
props0 = true;
}
}
}
}
}
}
}
}
if(!valid0){
const err43 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err43];
}
else {
vErrors.push(err43);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate23.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate23.evaluated = {"dynamicProps":true,"dynamicItems":false};


function validate22(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate22.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.schema === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "schema"},message:"must have required property '"+"schema"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "name") || (key0 === "schema"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err3 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/AttributeName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(func1(data0) < 1){
const err4 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/AttributeName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!pattern5.test(data0)){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/AttributeName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9_]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9_]*$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/AttributeName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.schema !== undefined){
if(!(validate23(data.schema, {instancePath:instancePath+"/schema",parentData:data,parentDataProperty:"schema",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
}
}
else {
const err7 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
validate22.errors = vErrors;
return errors === 0;
}
validate22.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema44 = {"type":"object","additionalProperties":false,"required":["schema","default"],"properties":{"schema":{"$ref":"#/$defs/ValueSchema"},"default":{"$ref":"#/$defs/JsonValue","description":"Literal fallback result. Contract Acceptance verifies that it satisfies schema."}}};
const schema45 = {"oneOf":[{"type":"null"},{"type":"boolean"},{"type":"number"},{"type":"string"},{"type":"array","items":{"$ref":"#/$defs/JsonValue"}},{"type":"object","additionalProperties":{"$ref":"#/$defs/JsonValue"}}]};
const wrapper2 = {validate: validate32};

function validate32(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate32.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(data !== null){
const err0 = {instancePath,schemaPath:"#/oneOf/0/type",keyword:"type",params:{type: "null"},message:"must be null"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
}
const _errs3 = errors;
if(typeof data !== "boolean"){
const err1 = {instancePath,schemaPath:"#/oneOf/1/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
var _valid0 = _errs3 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
}
const _errs5 = errors;
if(!((typeof data == "number") && (isFinite(data)))){
const err2 = {instancePath,schemaPath:"#/oneOf/2/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
var _valid0 = _errs5 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 2];
}
else {
if(_valid0){
valid0 = true;
passing0 = 2;
}
const _errs7 = errors;
if(typeof data !== "string"){
const err3 = {instancePath,schemaPath:"#/oneOf/3/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
var _valid0 = _errs7 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 3];
}
else {
if(_valid0){
valid0 = true;
passing0 = 3;
}
const _errs9 = errors;
if(Array.isArray(data)){
const len0 = data.length;
for(let i0=0; i0<len0; i0++){
if(!(wrapper2.validate(data[i0], {instancePath:instancePath+"/" + i0,parentData:data,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper2.validate.errors : vErrors.concat(wrapper2.validate.errors);
errors = vErrors.length;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/oneOf/4/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
var _valid0 = _errs9 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 4];
}
else {
if(_valid0){
valid0 = true;
passing0 = 4;
var items1 = true;
}
const _errs12 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
for(const key0 in data){
if(!(wrapper2.validate(data[key0], {instancePath:instancePath+"/" + key0.replace(/~/g, "~0").replace(/\//g, "~1"),parentData:data,parentDataProperty:key0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper2.validate.errors : vErrors.concat(wrapper2.validate.errors);
errors = vErrors.length;
}
}
}
else {
const err5 = {instancePath,schemaPath:"#/oneOf/5/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
var _valid0 = _errs12 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 5];
}
else {
if(_valid0){
valid0 = true;
passing0 = 5;
var props2 = true;
}
}
}
}
}
}
if(!valid0){
const err6 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate32.errors = vErrors;
evaluated0.props = props2;
evaluated0.items = items1;
return errors === 0;
}
validate32.evaluated = {"dynamicProps":true,"dynamicItems":true};


function validate30(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate30.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.schema === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "schema"},message:"must have required property '"+"schema"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.default === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "default"},message:"must have required property '"+"default"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "schema") || (key0 === "default"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.schema !== undefined){
if(!(validate23(data.schema, {instancePath:instancePath+"/schema",parentData:data,parentDataProperty:"schema",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
}
if(data.default !== undefined){
if(!(validate32(data.default, {instancePath:instancePath+"/default",parentData:data,parentDataProperty:"default",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate32.errors : vErrors.concat(validate32.errors);
errors = vErrors.length;
}
}
}
else {
const err3 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
validate30.errors = vErrors;
return errors === 0;
}
validate30.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema46 = {"type":"object","additionalProperties":false,"required":["rules"],"properties":{"rules":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/AuthoredRule"},"description":"Ordered first-match-wins rules. Rule names must be unique."}}};
const schema47 = {"type":"object","additionalProperties":false,"required":["name","when","return"],"properties":{"name":{"$ref":"#/$defs/MemberName"},"description":{"type":"string"},"when":{"$ref":"#/$defs/AuthoredWhen"},"return":{"$ref":"#/$defs/RuleReturn"}}};
const schema48 = {"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9._-]*$"};
const schema49 = {"oneOf":[{"$ref":"#/$defs/NaturalLanguageWhen"},{"$ref":"#/$defs/ExpressionWhen"}]};
const schema50 = {"type":"object","additionalProperties":false,"required":["condition"],"properties":{"condition":{"type":"string","minLength":1}}};
const schema51 = {"type":"object","additionalProperties":false,"required":["expression"],"properties":{"expression":{"$ref":"#/$defs/Expression"}}};
const schema52 = {"type":"string","minLength":1,"description":"An expression in the DecisionContract's expression_syntax."};

function validate38(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate38.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.expression === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "expression"},message:"must have required property '"+"expression"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!(key0 === "expression")){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.expression !== undefined){
let data0 = data.expression;
if(typeof data0 === "string"){
if(func1(data0) < 1){
const err2 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
else {
const err3 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
validate38.errors = vErrors;
return errors === 0;
}
validate38.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate37(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate37.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.condition === undefined){
const err0 = {instancePath,schemaPath:"#/$defs/NaturalLanguageWhen/required",keyword:"required",params:{missingProperty: "condition"},message:"must have required property '"+"condition"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!(key0 === "condition")){
const err1 = {instancePath,schemaPath:"#/$defs/NaturalLanguageWhen/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.condition !== undefined){
let data0 = data.condition;
if(typeof data0 === "string"){
if(func1(data0) < 1){
const err2 = {instancePath:instancePath+"/condition",schemaPath:"#/$defs/NaturalLanguageWhen/properties/condition/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
else {
const err3 = {instancePath:instancePath+"/condition",schemaPath:"#/$defs/NaturalLanguageWhen/properties/condition/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/$defs/NaturalLanguageWhen/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs7 = errors;
if(!(validate38(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate38.errors : vErrors.concat(validate38.errors);
errors = vErrors.length;
}
var _valid0 = _errs7 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
}
if(!valid0){
const err5 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate37.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate37.evaluated = {"dynamicProps":true,"dynamicItems":false};

const schema53 = {"oneOf":[{"$ref":"#/$defs/LiteralReturn"},{"$ref":"#/$defs/ExpressionReturn"}]};
const schema54 = {"type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"$ref":"#/$defs/JsonValue"}}};

function validate42(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate42.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.value === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "value"},message:"must have required property '"+"value"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!(key0 === "value")){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.value !== undefined){
if(!(validate32(data.value, {instancePath:instancePath+"/value",parentData:data,parentDataProperty:"value",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate32.errors : vErrors.concat(validate32.errors);
errors = vErrors.length;
}
}
}
else {
const err2 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
validate42.errors = vErrors;
return errors === 0;
}
validate42.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema55 = {"type":"object","additionalProperties":false,"required":["expression"],"properties":{"expression":{"$ref":"#/$defs/Expression"}}};

function validate45(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate45.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.expression === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "expression"},message:"must have required property '"+"expression"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!(key0 === "expression")){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.expression !== undefined){
let data0 = data.expression;
if(typeof data0 === "string"){
if(func1(data0) < 1){
const err2 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
else {
const err3 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
validate45.errors = vErrors;
return errors === 0;
}
validate45.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate41(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate41.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(!(validate42(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate42.errors : vErrors.concat(validate42.errors);
errors = vErrors.length;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs2 = errors;
if(!(validate45(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate45.errors : vErrors.concat(validate45.errors);
errors = vErrors.length;
}
var _valid0 = _errs2 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
}
if(!valid0){
const err0 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate41.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate41.evaluated = {"dynamicProps":true,"dynamicItems":false};


function validate36(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate36.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.when === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "when"},message:"must have required property '"+"when"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.return === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "return"},message:"must have required property '"+"return"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "name") || (key0 === "description")) || (key0 === "when")) || (key0 === "return"))){
const err3 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err4 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(func1(data0) < 1){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!pattern4.test(data0)){
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
else {
const err7 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err8 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.when !== undefined){
if(!(validate37(data.when, {instancePath:instancePath+"/when",parentData:data,parentDataProperty:"when",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate37.errors : vErrors.concat(validate37.errors);
errors = vErrors.length;
}
}
if(data.return !== undefined){
if(!(validate41(data.return, {instancePath:instancePath+"/return",parentData:data,parentDataProperty:"return",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate41.errors : vErrors.concat(validate41.errors);
errors = vErrors.length;
}
}
}
else {
const err9 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
validate36.errors = vErrors;
return errors === 0;
}
validate36.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate35(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate35.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.rules === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "rules"},message:"must have required property '"+"rules"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!(key0 === "rules")){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.rules !== undefined){
let data0 = data.rules;
if(Array.isArray(data0)){
if(data0.length < 1){
const err2 = {instancePath:instancePath+"/rules",schemaPath:"#/properties/rules/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
const len0 = data0.length;
for(let i0=0; i0<len0; i0++){
if(!(validate36(data0[i0], {instancePath:instancePath+"/rules/" + i0,parentData:data0,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate36.errors : vErrors.concat(validate36.errors);
errors = vErrors.length;
}
}
}
else {
const err3 = {instancePath:instancePath+"/rules",schemaPath:"#/properties/rules/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
}
else {
const err4 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
validate35.errors = vErrors;
return errors === 0;
}
validate35.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema57 = {"type":"object","additionalProperties":false,"required":["policy","evidence","objective"],"properties":{"policy":{"$ref":"#/$defs/LearningPolicy"},"evidence":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/Evidence"},"description":"One logical observed value per entry. Evidence names must be unique; order is non-semantic."},"objective":{"$ref":"#/$defs/LearningObjective"}}};
const schema58 = {"type":"object","additionalProperties":false,"required":["mode","evaluate"],"properties":{"mode":{"const":"auto-activation","description":"Automatically attempt atomic activation after an evidence-generated candidate passes validation and a current-contract check."},"evaluate":{"$ref":"#/$defs/LearningEvaluationPolicy"}}};
const schema59 = {"type":"object","additionalProperties":false,"required":["interval"],"properties":{"interval":{"type":"string","pattern":"^P(?=\\d|T\\d)(?:\\d+Y)?(?:\\d+M)?(?:\\d+W)?(?:\\d+D)?(?:T(?=\\d)(?:\\d+H)?(?:\\d+M)?(?:\\d+(?:\\.\\d+)?S)?)?$","description":"A positive ISO 8601 duration expressing the minimum delay between completed analysis attempts."}}};
const pattern7 = new RegExp("^P(?=\\d|T\\d)(?:\\d+Y)?(?:\\d+M)?(?:\\d+W)?(?:\\d+D)?(?:T(?=\\d)(?:\\d+H)?(?:\\d+M)?(?:\\d+(?:\\.\\d+)?S)?)?$", "u");

function validate51(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate51.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.mode === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "mode"},message:"must have required property '"+"mode"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.evaluate === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "evaluate"},message:"must have required property '"+"evaluate"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "mode") || (key0 === "evaluate"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.mode !== undefined){
if("auto-activation" !== data.mode){
const err3 = {instancePath:instancePath+"/mode",schemaPath:"#/properties/mode/const",keyword:"const",params:{allowedValue: "auto-activation"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.evaluate !== undefined){
let data1 = data.evaluate;
if(data1 && typeof data1 == "object" && !Array.isArray(data1)){
if(data1.interval === undefined){
const err4 = {instancePath:instancePath+"/evaluate",schemaPath:"#/$defs/LearningEvaluationPolicy/required",keyword:"required",params:{missingProperty: "interval"},message:"must have required property '"+"interval"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
for(const key1 in data1){
if(!(key1 === "interval")){
const err5 = {instancePath:instancePath+"/evaluate",schemaPath:"#/$defs/LearningEvaluationPolicy/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data1.interval !== undefined){
let data2 = data1.interval;
if(typeof data2 === "string"){
if(!pattern7.test(data2)){
const err6 = {instancePath:instancePath+"/evaluate/interval",schemaPath:"#/$defs/LearningEvaluationPolicy/properties/interval/pattern",keyword:"pattern",params:{pattern: "^P(?=\\d|T\\d)(?:\\d+Y)?(?:\\d+M)?(?:\\d+W)?(?:\\d+D)?(?:T(?=\\d)(?:\\d+H)?(?:\\d+M)?(?:\\d+(?:\\.\\d+)?S)?)?$"},message:"must match pattern \""+"^P(?=\\d|T\\d)(?:\\d+Y)?(?:\\d+M)?(?:\\d+W)?(?:\\d+D)?(?:T(?=\\d)(?:\\d+H)?(?:\\d+M)?(?:\\d+(?:\\.\\d+)?S)?)?$"+"\""};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
else {
const err7 = {instancePath:instancePath+"/evaluate/interval",schemaPath:"#/$defs/LearningEvaluationPolicy/properties/interval/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
}
else {
const err8 = {instancePath:instancePath+"/evaluate",schemaPath:"#/$defs/LearningEvaluationPolicy/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
}
else {
const err9 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
validate51.errors = vErrors;
return errors === 0;
}
validate51.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema60 = {"type":"object","additionalProperties":false,"required":["name","attribute","correlateBy","source"],"properties":{"name":{"$ref":"#/$defs/MemberName"},"description":{"type":"string"},"attribute":{"$ref":"#/$defs/AttributeName","description":"The contract attribute whose value semantics this observed evidence shares."},"correlateBy":{"type":"array","items":{"$ref":"#/$defs/AttributeName"},"maxItems":128,"uniqueItems":true,"description":"Unordered set of contract attributes whose OpenTelemetry locations are declared by source.correlation."},"source":{"$ref":"#/$defs/EvidenceSource"}}};
const func0 = require("ajv/dist/runtime/equal").default;
const schema64 = {"oneOf":[{"$ref":"#/$defs/MetricEvidenceSource"},{"$ref":"#/$defs/LogEvidenceSource"},{"$ref":"#/$defs/SpanEvidenceSource"},{"$ref":"#/$defs/SpanEventEvidenceSource"}],"description":"The one application-owned OpenTelemetry signal selected for this evidence declaration."};
const schema65 = {"type":"object","additionalProperties":false,"required":["kind","scope","name","metricKind","unit","correlation"],"properties":{"kind":{"const":"metric"},"scope":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive InstrumentationScope name."},"name":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive metric name."},"metricKind":{"type":"string","enum":["gauge","sum","histogram","exponentialHistogram","summary"]},"unit":{"type":"string","maxLength":128,"description":"Exact case-sensitive metric unit. Use an empty string for a metric with no unit."},"correlation":{"$ref":"#/$defs/EvidenceCorrelationMap"}}};
const schema66 = {"type":"object","propertyNames":{"$ref":"#/$defs/AttributeName"},"additionalProperties":{"$ref":"#/$defs/EvidenceCorrelationAttribute"},"maxProperties":128,"description":"Maps every correlateBy contract attribute to one exact OpenTelemetry attribute location and key."};
const schema68 = {"type":"object","additionalProperties":false,"required":["location","attribute"],"properties":{"location":{"type":"string","enum":["resource","scope","signal","parentSpan"],"description":"The OTLP envelope level containing the correlation attribute. parentSpan is valid only for spanEvent sources."},"attribute":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive OpenTelemetry attribute key."}}};

function validate56(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate56.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(Object.keys(data).length > 128){
const err0 = {instancePath,schemaPath:"#/maxProperties",keyword:"maxProperties",params:{limit: 128},message:"must NOT have more than 128 properties"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
const _errs1 = errors;
if(typeof key0 === "string"){
if(func1(key0) > 128){
const err1 = {instancePath,schemaPath:"#/$defs/AttributeName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters",propertyName:key0};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(func1(key0) < 1){
const err2 = {instancePath,schemaPath:"#/$defs/AttributeName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters",propertyName:key0};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(!pattern5.test(key0)){
const err3 = {instancePath,schemaPath:"#/$defs/AttributeName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9_]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9_]*$"+"\"",propertyName:key0};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
else {
const err4 = {instancePath,schemaPath:"#/$defs/AttributeName/type",keyword:"type",params:{type: "string"},message:"must be string",propertyName:key0};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
var valid0 = _errs1 === errors;
if(!valid0){
const err5 = {instancePath,schemaPath:"#/propertyNames",keyword:"propertyNames",params:{propertyName: key0},message:"property name must be valid"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
for(const key1 in data){
let data0 = data[key1];
if(data0 && typeof data0 == "object" && !Array.isArray(data0)){
if(data0.location === undefined){
const err6 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1"),schemaPath:"#/$defs/EvidenceCorrelationAttribute/required",keyword:"required",params:{missingProperty: "location"},message:"must have required property '"+"location"+"'"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(data0.attribute === undefined){
const err7 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1"),schemaPath:"#/$defs/EvidenceCorrelationAttribute/required",keyword:"required",params:{missingProperty: "attribute"},message:"must have required property '"+"attribute"+"'"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
for(const key2 in data0){
if(!((key2 === "location") || (key2 === "attribute"))){
const err8 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1"),schemaPath:"#/$defs/EvidenceCorrelationAttribute/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key2},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data0.location !== undefined){
let data1 = data0.location;
if(typeof data1 !== "string"){
const err9 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1")+"/location",schemaPath:"#/$defs/EvidenceCorrelationAttribute/properties/location/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(!((((data1 === "resource") || (data1 === "scope")) || (data1 === "signal")) || (data1 === "parentSpan"))){
const err10 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1")+"/location",schemaPath:"#/$defs/EvidenceCorrelationAttribute/properties/location/enum",keyword:"enum",params:{allowedValues: schema68.properties.location.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data0.attribute !== undefined){
let data2 = data0.attribute;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err11 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1")+"/attribute",schemaPath:"#/$defs/EvidenceCorrelationAttribute/properties/attribute/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(func1(data2) < 1){
const err12 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1")+"/attribute",schemaPath:"#/$defs/EvidenceCorrelationAttribute/properties/attribute/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
else {
const err13 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1")+"/attribute",schemaPath:"#/$defs/EvidenceCorrelationAttribute/properties/attribute/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
}
else {
const err14 = {instancePath:instancePath+"/" + key1.replace(/~/g, "~0").replace(/\//g, "~1"),schemaPath:"#/$defs/EvidenceCorrelationAttribute/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
}
else {
const err15 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
validate56.errors = vErrors;
return errors === 0;
}
validate56.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate55(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate55.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.kind === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "kind"},message:"must have required property '"+"kind"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.scope === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "scope"},message:"must have required property '"+"scope"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.name === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.metricKind === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "metricKind"},message:"must have required property '"+"metricKind"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(data.unit === undefined){
const err4 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "unit"},message:"must have required property '"+"unit"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(data.correlation === undefined){
const err5 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "correlation"},message:"must have required property '"+"correlation"+"'"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
for(const key0 in data){
if(!((((((key0 === "kind") || (key0 === "scope")) || (key0 === "name")) || (key0 === "metricKind")) || (key0 === "unit")) || (key0 === "correlation"))){
const err6 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.kind !== undefined){
if("metric" !== data.kind){
const err7 = {instancePath:instancePath+"/kind",schemaPath:"#/properties/kind/const",keyword:"const",params:{allowedValue: "metric"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.scope !== undefined){
let data1 = data.scope;
if(typeof data1 === "string"){
if(func1(data1) > 256){
const err8 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(func1(data1) < 1){
const err9 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
else {
const err10 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.name !== undefined){
let data2 = data.name;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err11 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(func1(data2) < 1){
const err12 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
else {
const err13 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.metricKind !== undefined){
let data3 = data.metricKind;
if(typeof data3 !== "string"){
const err14 = {instancePath:instancePath+"/metricKind",schemaPath:"#/properties/metricKind/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
if(!(((((data3 === "gauge") || (data3 === "sum")) || (data3 === "histogram")) || (data3 === "exponentialHistogram")) || (data3 === "summary"))){
const err15 = {instancePath:instancePath+"/metricKind",schemaPath:"#/properties/metricKind/enum",keyword:"enum",params:{allowedValues: schema65.properties.metricKind.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.unit !== undefined){
let data4 = data.unit;
if(typeof data4 === "string"){
if(func1(data4) > 128){
const err16 = {instancePath:instancePath+"/unit",schemaPath:"#/properties/unit/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
else {
const err17 = {instancePath:instancePath+"/unit",schemaPath:"#/properties/unit/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
if(data.correlation !== undefined){
if(!(validate56(data.correlation, {instancePath:instancePath+"/correlation",parentData:data,parentDataProperty:"correlation",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
errors = vErrors.length;
}
}
}
else {
const err18 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
validate55.errors = vErrors;
return errors === 0;
}
validate55.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema69 = {"type":"object","additionalProperties":false,"required":["kind","scope","name","correlation"],"properties":{"kind":{"const":"log"},"scope":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive InstrumentationScope name."},"name":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive LogRecord event name."},"correlation":{"$ref":"#/$defs/EvidenceCorrelationMap"}}};

function validate59(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate59.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.kind === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "kind"},message:"must have required property '"+"kind"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.scope === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "scope"},message:"must have required property '"+"scope"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.name === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.correlation === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "correlation"},message:"must have required property '"+"correlation"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "kind") || (key0 === "scope")) || (key0 === "name")) || (key0 === "correlation"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.kind !== undefined){
if("log" !== data.kind){
const err5 = {instancePath:instancePath+"/kind",schemaPath:"#/properties/kind/const",keyword:"const",params:{allowedValue: "log"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.scope !== undefined){
let data1 = data.scope;
if(typeof data1 === "string"){
if(func1(data1) > 256){
const err6 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(func1(data1) < 1){
const err7 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.name !== undefined){
let data2 = data.name;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err9 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(func1(data2) < 1){
const err10 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
else {
const err11 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.correlation !== undefined){
if(!(validate56(data.correlation, {instancePath:instancePath+"/correlation",parentData:data,parentDataProperty:"correlation",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
errors = vErrors.length;
}
}
}
else {
const err12 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
validate59.errors = vErrors;
return errors === 0;
}
validate59.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema70 = {"type":"object","additionalProperties":false,"required":["kind","scope","name","correlation"],"properties":{"kind":{"const":"span"},"scope":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive InstrumentationScope name."},"name":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive span name."},"correlation":{"$ref":"#/$defs/EvidenceCorrelationMap"}}};

function validate62(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate62.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.kind === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "kind"},message:"must have required property '"+"kind"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.scope === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "scope"},message:"must have required property '"+"scope"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.name === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.correlation === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "correlation"},message:"must have required property '"+"correlation"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "kind") || (key0 === "scope")) || (key0 === "name")) || (key0 === "correlation"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.kind !== undefined){
if("span" !== data.kind){
const err5 = {instancePath:instancePath+"/kind",schemaPath:"#/properties/kind/const",keyword:"const",params:{allowedValue: "span"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.scope !== undefined){
let data1 = data.scope;
if(typeof data1 === "string"){
if(func1(data1) > 256){
const err6 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(func1(data1) < 1){
const err7 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.name !== undefined){
let data2 = data.name;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err9 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(func1(data2) < 1){
const err10 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
else {
const err11 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.correlation !== undefined){
if(!(validate56(data.correlation, {instancePath:instancePath+"/correlation",parentData:data,parentDataProperty:"correlation",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
errors = vErrors.length;
}
}
}
else {
const err12 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
validate62.errors = vErrors;
return errors === 0;
}
validate62.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema71 = {"type":"object","additionalProperties":false,"required":["kind","scope","spanName","name","correlation"],"properties":{"kind":{"const":"spanEvent"},"scope":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive InstrumentationScope name."},"spanName":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive parent span name."},"name":{"type":"string","minLength":1,"maxLength":256,"description":"Exact case-sensitive span-event name."},"correlation":{"$ref":"#/$defs/EvidenceCorrelationMap"}}};

function validate65(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate65.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.kind === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "kind"},message:"must have required property '"+"kind"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.scope === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "scope"},message:"must have required property '"+"scope"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.spanName === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "spanName"},message:"must have required property '"+"spanName"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.name === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(data.correlation === undefined){
const err4 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "correlation"},message:"must have required property '"+"correlation"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
for(const key0 in data){
if(!(((((key0 === "kind") || (key0 === "scope")) || (key0 === "spanName")) || (key0 === "name")) || (key0 === "correlation"))){
const err5 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.kind !== undefined){
if("spanEvent" !== data.kind){
const err6 = {instancePath:instancePath+"/kind",schemaPath:"#/properties/kind/const",keyword:"const",params:{allowedValue: "spanEvent"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.scope !== undefined){
let data1 = data.scope;
if(typeof data1 === "string"){
if(func1(data1) > 256){
const err7 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(func1(data1) < 1){
const err8 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
else {
const err9 = {instancePath:instancePath+"/scope",schemaPath:"#/properties/scope/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.spanName !== undefined){
let data2 = data.spanName;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err10 = {instancePath:instancePath+"/spanName",schemaPath:"#/properties/spanName/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(func1(data2) < 1){
const err11 = {instancePath:instancePath+"/spanName",schemaPath:"#/properties/spanName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
else {
const err12 = {instancePath:instancePath+"/spanName",schemaPath:"#/properties/spanName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data.name !== undefined){
let data3 = data.name;
if(typeof data3 === "string"){
if(func1(data3) > 256){
const err13 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(func1(data3) < 1){
const err14 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
else {
const err15 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.correlation !== undefined){
if(!(validate56(data.correlation, {instancePath:instancePath+"/correlation",parentData:data,parentDataProperty:"correlation",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
errors = vErrors.length;
}
}
}
else {
const err16 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
validate65.errors = vErrors;
return errors === 0;
}
validate65.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate54(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate54.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(!(validate55(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate55.errors : vErrors.concat(validate55.errors);
errors = vErrors.length;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs2 = errors;
if(!(validate59(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate59.errors : vErrors.concat(validate59.errors);
errors = vErrors.length;
}
var _valid0 = _errs2 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
const _errs3 = errors;
if(!(validate62(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate62.errors : vErrors.concat(validate62.errors);
errors = vErrors.length;
}
var _valid0 = _errs3 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 2];
}
else {
if(_valid0){
valid0 = true;
passing0 = 2;
if(props0 !== true){
props0 = true;
}
}
const _errs4 = errors;
if(!(validate65(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate65.errors : vErrors.concat(validate65.errors);
errors = vErrors.length;
}
var _valid0 = _errs4 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 3];
}
else {
if(_valid0){
valid0 = true;
passing0 = 3;
if(props0 !== true){
props0 = true;
}
}
}
}
}
if(!valid0){
const err0 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate54.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate54.evaluated = {"dynamicProps":true,"dynamicItems":false};


function validate53(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate53.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.attribute === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "attribute"},message:"must have required property '"+"attribute"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.correlateBy === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "correlateBy"},message:"must have required property '"+"correlateBy"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.source === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "source"},message:"must have required property '"+"source"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!(((((key0 === "name") || (key0 === "description")) || (key0 === "attribute")) || (key0 === "correlateBy")) || (key0 === "source"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(func1(data0) < 1){
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(!pattern4.test(data0)){
const err7 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err9 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.attribute !== undefined){
let data2 = data.attribute;
if(typeof data2 === "string"){
if(func1(data2) > 128){
const err10 = {instancePath:instancePath+"/attribute",schemaPath:"#/$defs/AttributeName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(func1(data2) < 1){
const err11 = {instancePath:instancePath+"/attribute",schemaPath:"#/$defs/AttributeName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(!pattern5.test(data2)){
const err12 = {instancePath:instancePath+"/attribute",schemaPath:"#/$defs/AttributeName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9_]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9_]*$"+"\""};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
else {
const err13 = {instancePath:instancePath+"/attribute",schemaPath:"#/$defs/AttributeName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.correlateBy !== undefined){
let data3 = data.correlateBy;
if(Array.isArray(data3)){
if(data3.length > 128){
const err14 = {instancePath:instancePath+"/correlateBy",schemaPath:"#/properties/correlateBy/maxItems",keyword:"maxItems",params:{limit: 128},message:"must NOT have more than 128 items"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
const len0 = data3.length;
for(let i0=0; i0<len0; i0++){
let data4 = data3[i0];
if(typeof data4 === "string"){
if(func1(data4) > 128){
const err15 = {instancePath:instancePath+"/correlateBy/" + i0,schemaPath:"#/$defs/AttributeName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
if(func1(data4) < 1){
const err16 = {instancePath:instancePath+"/correlateBy/" + i0,schemaPath:"#/$defs/AttributeName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
if(!pattern5.test(data4)){
const err17 = {instancePath:instancePath+"/correlateBy/" + i0,schemaPath:"#/$defs/AttributeName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9_]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9_]*$"+"\""};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
else {
const err18 = {instancePath:instancePath+"/correlateBy/" + i0,schemaPath:"#/$defs/AttributeName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
}
let i1 = data3.length;
let j0;
if(i1 > 1){
outer0:
for(;i1--;){
for(j0 = i1; j0--;){
if(func0(data3[i1], data3[j0])){
const err19 = {instancePath:instancePath+"/correlateBy",schemaPath:"#/properties/correlateBy/uniqueItems",keyword:"uniqueItems",params:{i: i1, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i1+" are identical)"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
break outer0;
}
}
}
}
}
else {
const err20 = {instancePath:instancePath+"/correlateBy",schemaPath:"#/properties/correlateBy/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
}
if(data.source !== undefined){
if(!(validate54(data.source, {instancePath:instancePath+"/source",parentData:data,parentDataProperty:"source",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate54.errors : vErrors.concat(validate54.errors);
errors = vErrors.length;
}
}
}
else {
const err21 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
validate53.errors = vErrors;
return errors === 0;
}
validate53.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema72 = {"type":"object","additionalProperties":false,"required":["primary"],"properties":{"primary":{"$ref":"#/$defs/PrimaryObjective"},"guardrails":{"type":"array","items":{"$ref":"#/$defs/Guardrail"},"description":"Guardrail names must be unique; order is non-semantic."}}};
const schema73 = {"type":"object","additionalProperties":false,"required":["evidence","direction"],"properties":{"evidence":{"$ref":"#/$defs/MemberName"},"direction":{"type":"string","enum":["minimize","maximize"]}}};

function validate71(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate71.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.evidence === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "evidence"},message:"must have required property '"+"evidence"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.direction === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "direction"},message:"must have required property '"+"direction"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "evidence") || (key0 === "direction"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.evidence !== undefined){
let data0 = data.evidence;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err3 = {instancePath:instancePath+"/evidence",schemaPath:"#/$defs/MemberName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(func1(data0) < 1){
const err4 = {instancePath:instancePath+"/evidence",schemaPath:"#/$defs/MemberName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!pattern4.test(data0)){
const err5 = {instancePath:instancePath+"/evidence",schemaPath:"#/$defs/MemberName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/evidence",schemaPath:"#/$defs/MemberName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.direction !== undefined){
let data1 = data.direction;
if(typeof data1 !== "string"){
const err7 = {instancePath:instancePath+"/direction",schemaPath:"#/properties/direction/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(!((data1 === "minimize") || (data1 === "maximize"))){
const err8 = {instancePath:instancePath+"/direction",schemaPath:"#/properties/direction/enum",keyword:"enum",params:{allowedValues: schema73.properties.direction.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
}
else {
const err9 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
validate71.errors = vErrors;
return errors === 0;
}
validate71.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema75 = {"type":"object","additionalProperties":false,"required":["name","expression"],"properties":{"name":{"$ref":"#/$defs/MemberName"},"description":{"type":"string"},"expression":{"$ref":"#/$defs/Expression"}}};

function validate73(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate73.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.expression === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "expression"},message:"must have required property '"+"expression"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "name") || (key0 === "description")) || (key0 === "expression"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err3 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(func1(data0) < 1){
const err4 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!pattern4.test(data0)){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/MemberName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.description !== undefined){
if(typeof data.description !== "string"){
const err7 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.expression !== undefined){
let data2 = data.expression;
if(typeof data2 === "string"){
if(func1(data2) < 1){
const err8 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
else {
const err9 = {instancePath:instancePath+"/expression",schemaPath:"#/$defs/Expression/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
}
else {
const err10 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
validate73.errors = vErrors;
return errors === 0;
}
validate73.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate70(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate70.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.primary === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "primary"},message:"must have required property '"+"primary"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "primary") || (key0 === "guardrails"))){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.primary !== undefined){
if(!(validate71(data.primary, {instancePath:instancePath+"/primary",parentData:data,parentDataProperty:"primary",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate71.errors : vErrors.concat(validate71.errors);
errors = vErrors.length;
}
}
if(data.guardrails !== undefined){
let data1 = data.guardrails;
if(Array.isArray(data1)){
const len0 = data1.length;
for(let i0=0; i0<len0; i0++){
if(!(validate73(data1[i0], {instancePath:instancePath+"/guardrails/" + i0,parentData:data1,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate73.errors : vErrors.concat(validate73.errors);
errors = vErrors.length;
}
}
}
else {
const err2 = {instancePath:instancePath+"/guardrails",schemaPath:"#/properties/guardrails/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
}
else {
const err3 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
validate70.errors = vErrors;
return errors === 0;
}
validate70.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate50(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate50.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.policy === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "policy"},message:"must have required property '"+"policy"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.evidence === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "evidence"},message:"must have required property '"+"evidence"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.objective === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "objective"},message:"must have required property '"+"objective"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "policy") || (key0 === "evidence")) || (key0 === "objective"))){
const err3 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.policy !== undefined){
if(!(validate51(data.policy, {instancePath:instancePath+"/policy",parentData:data,parentDataProperty:"policy",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate51.errors : vErrors.concat(validate51.errors);
errors = vErrors.length;
}
}
if(data.evidence !== undefined){
let data1 = data.evidence;
if(Array.isArray(data1)){
if(data1.length < 1){
const err4 = {instancePath:instancePath+"/evidence",schemaPath:"#/properties/evidence/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
const len0 = data1.length;
for(let i0=0; i0<len0; i0++){
if(!(validate53(data1[i0], {instancePath:instancePath+"/evidence/" + i0,parentData:data1,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate53.errors : vErrors.concat(validate53.errors);
errors = vErrors.length;
}
}
}
else {
const err5 = {instancePath:instancePath+"/evidence",schemaPath:"#/properties/evidence/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.objective !== undefined){
if(!(validate70(data.objective, {instancePath:instancePath+"/objective",parentData:data,parentDataProperty:"objective",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate70.errors : vErrors.concat(validate70.errors);
errors = vErrors.length;
}
}
}
else {
const err6 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
validate50.errors = vErrors;
return errors === 0;
}
validate50.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate21(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate21.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.expression_syntax === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "expression_syntax"},message:"must have required property '"+"expression_syntax"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.attributes === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "attributes"},message:"must have required property '"+"attributes"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.result === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "result"},message:"must have required property '"+"result"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((((key0 === "name") || (key0 === "expression_syntax")) || (key0 === "attributes")) || (key0 === "result")) || (key0 === "authoredExecutable")) || (key0 === "learning"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(func1(data0) < 1){
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(!pattern4.test(data0)){
const err7 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.expression_syntax !== undefined){
if("flaggo.cel/v1" !== data.expression_syntax){
const err9 = {instancePath:instancePath+"/expression_syntax",schemaPath:"#/properties/expression_syntax/const",keyword:"const",params:{allowedValue: "flaggo.cel/v1"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.attributes !== undefined){
let data2 = data.attributes;
if(Array.isArray(data2)){
if(data2.length > 128){
const err10 = {instancePath:instancePath+"/attributes",schemaPath:"#/properties/attributes/maxItems",keyword:"maxItems",params:{limit: 128},message:"must NOT have more than 128 items"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
if(!(validate22(data2[i0], {instancePath:instancePath+"/attributes/" + i0,parentData:data2,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate22.errors : vErrors.concat(validate22.errors);
errors = vErrors.length;
}
}
}
else {
const err11 = {instancePath:instancePath+"/attributes",schemaPath:"#/properties/attributes/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.result !== undefined){
if(!(validate30(data.result, {instancePath:instancePath+"/result",parentData:data,parentDataProperty:"result",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate30.errors : vErrors.concat(validate30.errors);
errors = vErrors.length;
}
}
if(data.authoredExecutable !== undefined){
if(!(validate35(data.authoredExecutable, {instancePath:instancePath+"/authoredExecutable",parentData:data,parentDataProperty:"authoredExecutable",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate35.errors : vErrors.concat(validate35.errors);
errors = vErrors.length;
}
}
if(data.learning !== undefined){
if(!(validate50(data.learning, {instancePath:instancePath+"/learning",parentData:data,parentDataProperty:"learning",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate50.errors : vErrors.concat(validate50.errors);
errors = vErrors.length;
}
}
}
else {
const err12 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
validate21.errors = vErrors;
return errors === 0;
}
validate21.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

export const validateDecisionContractValidationResult = validate77;
const schema78 = {"oneOf":[{"$ref":"#/$defs/ValidDecisionContract"},{"$ref":"#/$defs/InvalidDecisionContract"}]};
const schema79 = {"type":"object","additionalProperties":false,"required":["status","contractDigest","issues"],"properties":{"status":{"const":"valid"},"contractDigest":{"$ref":"#/$defs/Sha256Digest"},"issues":{"type":"array","items":{"$ref":"#/$defs/WarningIssue"}}}};
const schema80 = {"type":"string","pattern":"^sha256:[0-9a-f]{64}$"};
const pattern14 = new RegExp("^sha256:[0-9a-f]{64}$", "u");
const schema81 = {"allOf":[{"$ref":"#/$defs/ValidationIssue"},{"properties":{"severity":{"const":"warning"}}}]};
const schema82 = {"type":"object","additionalProperties":false,"required":["code","severity","path","message"],"properties":{"code":{"type":"string","minLength":1,"pattern":"^[a-z][a-z0-9-]*$"},"severity":{"type":"string","enum":["error","warning"]},"path":{"type":"string","description":"JSON Pointer to the affected contract location; an empty string identifies the root."},"message":{"type":"string","minLength":1}}};
const pattern15 = new RegExp("^[a-z][a-z0-9-]*$", "u");

function validate79(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate79.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.code === undefined){
const err0 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "code"},message:"must have required property '"+"code"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.severity === undefined){
const err1 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "severity"},message:"must have required property '"+"severity"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.path === undefined){
const err2 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "path"},message:"must have required property '"+"path"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.message === undefined){
const err3 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "message"},message:"must have required property '"+"message"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "code") || (key0 === "severity")) || (key0 === "path")) || (key0 === "message"))){
const err4 = {instancePath,schemaPath:"#/$defs/ValidationIssue/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.code !== undefined){
let data0 = data.code;
if(typeof data0 === "string"){
if(func1(data0) < 1){
const err5 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!pattern15.test(data0)){
const err6 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9-]*$"},message:"must match pattern \""+"^[a-z][a-z0-9-]*$"+"\""};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
else {
const err7 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.severity !== undefined){
let data1 = data.severity;
if(typeof data1 !== "string"){
const err8 = {instancePath:instancePath+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!((data1 === "error") || (data1 === "warning"))){
const err9 = {instancePath:instancePath+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/enum",keyword:"enum",params:{allowedValues: schema82.properties.severity.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.path !== undefined){
if(typeof data.path !== "string"){
const err10 = {instancePath:instancePath+"/path",schemaPath:"#/$defs/ValidationIssue/properties/path/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.message !== undefined){
let data3 = data.message;
if(typeof data3 === "string"){
if(func1(data3) < 1){
const err11 = {instancePath:instancePath+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
else {
const err12 = {instancePath:instancePath+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
}
else {
const err13 = {instancePath,schemaPath:"#/$defs/ValidationIssue/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.severity !== undefined){
if("warning" !== data.severity){
const err14 = {instancePath:instancePath+"/severity",schemaPath:"#/allOf/1/properties/severity/const",keyword:"const",params:{allowedValue: "warning"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
}
validate79.errors = vErrors;
return errors === 0;
}
validate79.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate78(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate78.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.status === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "status"},message:"must have required property '"+"status"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.contractDigest === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contractDigest"},message:"must have required property '"+"contractDigest"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.issues === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "issues"},message:"must have required property '"+"issues"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "status") || (key0 === "contractDigest")) || (key0 === "issues"))){
const err3 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.status !== undefined){
if("valid" !== data.status){
const err4 = {instancePath:instancePath+"/status",schemaPath:"#/properties/status/const",keyword:"const",params:{allowedValue: "valid"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.contractDigest !== undefined){
let data1 = data.contractDigest;
if(typeof data1 === "string"){
if(!pattern14.test(data1)){
const err5 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.issues !== undefined){
let data2 = data.issues;
if(Array.isArray(data2)){
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
if(!(validate79(data2[i0], {instancePath:instancePath+"/issues/" + i0,parentData:data2,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate79.errors : vErrors.concat(validate79.errors);
errors = vErrors.length;
}
}
}
else {
const err7 = {instancePath:instancePath+"/issues",schemaPath:"#/properties/issues/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
}
else {
const err8 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
validate78.errors = vErrors;
return errors === 0;
}
validate78.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema83 = {"type":"object","additionalProperties":false,"required":["status","issues"],"properties":{"status":{"const":"invalid"},"issues":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/ValidationIssue"},"contains":{"$ref":"#/$defs/ErrorIssue"}}}};
const schema85 = {"allOf":[{"$ref":"#/$defs/ValidationIssue"},{"properties":{"severity":{"const":"error"}}}]};

function validate83(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate83.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.code === undefined){
const err0 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "code"},message:"must have required property '"+"code"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.severity === undefined){
const err1 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "severity"},message:"must have required property '"+"severity"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.path === undefined){
const err2 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "path"},message:"must have required property '"+"path"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.message === undefined){
const err3 = {instancePath,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "message"},message:"must have required property '"+"message"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "code") || (key0 === "severity")) || (key0 === "path")) || (key0 === "message"))){
const err4 = {instancePath,schemaPath:"#/$defs/ValidationIssue/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.code !== undefined){
let data0 = data.code;
if(typeof data0 === "string"){
if(func1(data0) < 1){
const err5 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!pattern15.test(data0)){
const err6 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9-]*$"},message:"must match pattern \""+"^[a-z][a-z0-9-]*$"+"\""};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
else {
const err7 = {instancePath:instancePath+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.severity !== undefined){
let data1 = data.severity;
if(typeof data1 !== "string"){
const err8 = {instancePath:instancePath+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!((data1 === "error") || (data1 === "warning"))){
const err9 = {instancePath:instancePath+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/enum",keyword:"enum",params:{allowedValues: schema82.properties.severity.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.path !== undefined){
if(typeof data.path !== "string"){
const err10 = {instancePath:instancePath+"/path",schemaPath:"#/$defs/ValidationIssue/properties/path/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.message !== undefined){
let data3 = data.message;
if(typeof data3 === "string"){
if(func1(data3) < 1){
const err11 = {instancePath:instancePath+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
else {
const err12 = {instancePath:instancePath+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
}
else {
const err13 = {instancePath,schemaPath:"#/$defs/ValidationIssue/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.severity !== undefined){
if("error" !== data.severity){
const err14 = {instancePath:instancePath+"/severity",schemaPath:"#/allOf/1/properties/severity/const",keyword:"const",params:{allowedValue: "error"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
}
validate83.errors = vErrors;
return errors === 0;
}
validate83.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate82(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate82.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.status === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "status"},message:"must have required property '"+"status"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.issues === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "issues"},message:"must have required property '"+"issues"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "status") || (key0 === "issues"))){
const err2 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.status !== undefined){
if("invalid" !== data.status){
const err3 = {instancePath:instancePath+"/status",schemaPath:"#/properties/status/const",keyword:"const",params:{allowedValue: "invalid"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.issues !== undefined){
let data1 = data.issues;
if(Array.isArray(data1)){
if(data1.length < 1){
const err4 = {instancePath:instancePath+"/issues",schemaPath:"#/properties/issues/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
const len0 = data1.length;
for(let i0=0; i0<len0; i0++){
let data2 = data1[i0];
if(data2 && typeof data2 == "object" && !Array.isArray(data2)){
if(data2.code === undefined){
const err5 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "code"},message:"must have required property '"+"code"+"'"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data2.severity === undefined){
const err6 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "severity"},message:"must have required property '"+"severity"+"'"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(data2.path === undefined){
const err7 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "path"},message:"must have required property '"+"path"+"'"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(data2.message === undefined){
const err8 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/required",keyword:"required",params:{missingProperty: "message"},message:"must have required property '"+"message"+"'"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
for(const key1 in data2){
if(!((((key1 === "code") || (key1 === "severity")) || (key1 === "path")) || (key1 === "message"))){
const err9 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data2.code !== undefined){
let data3 = data2.code;
if(typeof data3 === "string"){
if(func1(data3) < 1){
const err10 = {instancePath:instancePath+"/issues/" + i0+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(!pattern15.test(data3)){
const err11 = {instancePath:instancePath+"/issues/" + i0+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9-]*$"},message:"must match pattern \""+"^[a-z][a-z0-9-]*$"+"\""};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
else {
const err12 = {instancePath:instancePath+"/issues/" + i0+"/code",schemaPath:"#/$defs/ValidationIssue/properties/code/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data2.severity !== undefined){
let data4 = data2.severity;
if(typeof data4 !== "string"){
const err13 = {instancePath:instancePath+"/issues/" + i0+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(!((data4 === "error") || (data4 === "warning"))){
const err14 = {instancePath:instancePath+"/issues/" + i0+"/severity",schemaPath:"#/$defs/ValidationIssue/properties/severity/enum",keyword:"enum",params:{allowedValues: schema82.properties.severity.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data2.path !== undefined){
if(typeof data2.path !== "string"){
const err15 = {instancePath:instancePath+"/issues/" + i0+"/path",schemaPath:"#/$defs/ValidationIssue/properties/path/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data2.message !== undefined){
let data6 = data2.message;
if(typeof data6 === "string"){
if(func1(data6) < 1){
const err16 = {instancePath:instancePath+"/issues/" + i0+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
else {
const err17 = {instancePath:instancePath+"/issues/" + i0+"/message",schemaPath:"#/$defs/ValidationIssue/properties/message/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
}
else {
const err18 = {instancePath:instancePath+"/issues/" + i0,schemaPath:"#/$defs/ValidationIssue/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
}
const _errs17 = errors;
const len1 = data1.length;
for(let i1=0; i1<len1; i1++){
const _errs18 = errors;
if(!(validate83(data1[i1], {instancePath:instancePath+"/issues/" + i1,parentData:data1,parentDataProperty:i1,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate83.errors : vErrors.concat(validate83.errors);
errors = vErrors.length;
}
var valid5 = _errs18 === errors;
if(valid5){
break;
}
}
if(!valid5){
const err19 = {instancePath:instancePath+"/issues",schemaPath:"#/properties/issues/contains",keyword:"contains",params:{minContains: 1},message:"must contain at least 1 valid item(s)"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
else {
errors = _errs17;
if(vErrors !== null){
if(_errs17){
vErrors.length = _errs17;
}
else {
vErrors = null;
}
}
}
}
else {
const err20 = {instancePath:instancePath+"/issues",schemaPath:"#/properties/issues/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
}
}
else {
const err21 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
validate82.errors = vErrors;
return errors === 0;
}
validate82.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate77(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate77.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(!(validate78(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate78.errors : vErrors.concat(validate78.errors);
errors = vErrors.length;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs2 = errors;
if(!(validate82(data, {instancePath,parentData,parentDataProperty,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate82.errors : vErrors.concat(validate82.errors);
errors = vErrors.length;
}
var _valid0 = _errs2 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
}
if(!valid0){
const err0 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate77.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate77.evaluated = {"dynamicProps":true,"dynamicItems":false};

export const validateDecisionContractVersion = validate86;
const schema87 = {"type":"object","additionalProperties":false,"required":["name","contractDigest","status","acceptedAt","activeExecutableDigest","contract"],"properties":{"name":{"$ref":"#/$defs/DecisionName"},"contractDigest":{"$ref":"#/$defs/Sha256Digest"},"status":{"const":"ready","description":"A version is deployed only after its generated default executable has been activated."},"acceptedAt":{"type":"string","format":"date-time"},"activeExecutableDigest":{"$ref":"#/$defs/Sha256Digest"},"contract":{"$ref":"#/$defs/DecisionContract","description":"The immutable accepted contract document. Its name must equal the enclosing resource name."}}};
const formats0 = require("ajv-formats/dist/formats").fullFormats["date-time"];

function validate86(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate86.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.contractDigest === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contractDigest"},message:"must have required property '"+"contractDigest"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.status === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "status"},message:"must have required property '"+"status"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.acceptedAt === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "acceptedAt"},message:"must have required property '"+"acceptedAt"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(data.activeExecutableDigest === undefined){
const err4 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "activeExecutableDigest"},message:"must have required property '"+"activeExecutableDigest"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(data.contract === undefined){
const err5 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contract"},message:"must have required property '"+"contract"+"'"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
for(const key0 in data){
if(!((((((key0 === "name") || (key0 === "contractDigest")) || (key0 === "status")) || (key0 === "acceptedAt")) || (key0 === "activeExecutableDigest")) || (key0 === "contract"))){
const err6 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err7 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(func1(data0) < 1){
const err8 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!pattern4.test(data0)){
const err9 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
else {
const err10 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.contractDigest !== undefined){
let data1 = data.contractDigest;
if(typeof data1 === "string"){
if(!pattern14.test(data1)){
const err11 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
else {
const err12 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data.status !== undefined){
if("ready" !== data.status){
const err13 = {instancePath:instancePath+"/status",schemaPath:"#/properties/status/const",keyword:"const",params:{allowedValue: "ready"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.acceptedAt !== undefined){
let data3 = data.acceptedAt;
if(typeof data3 === "string"){
if(!(formats0.validate(data3))){
const err14 = {instancePath:instancePath+"/acceptedAt",schemaPath:"#/properties/acceptedAt/format",keyword:"format",params:{format: "date-time"},message:"must match format \""+"date-time"+"\""};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
else {
const err15 = {instancePath:instancePath+"/acceptedAt",schemaPath:"#/properties/acceptedAt/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.activeExecutableDigest !== undefined){
let data4 = data.activeExecutableDigest;
if(typeof data4 === "string"){
if(!pattern14.test(data4)){
const err16 = {instancePath:instancePath+"/activeExecutableDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
else {
const err17 = {instancePath:instancePath+"/activeExecutableDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
if(data.contract !== undefined){
if(!(validate21(data.contract, {instancePath:instancePath+"/contract",parentData:data,parentDataProperty:"contract",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate21.errors : vErrors.concat(validate21.errors);
errors = vErrors.length;
}
}
}
else {
const err18 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
validate86.errors = vErrors;
return errors === 0;
}
validate86.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

export const validateDecisionContractVersionList = validate88;
const schema91 = {"type":"object","additionalProperties":false,"required":["name","currentContractDigest","versions","nextCursor"],"properties":{"name":{"$ref":"#/$defs/DecisionName"},"currentContractDigest":{"$ref":"#/$defs/Sha256Digest"},"versions":{"type":"array","items":{"$ref":"#/$defs/DecisionContractVersionSummary"},"description":"Versions ordered by acceptedAt descending and then contractDigest descending."},"nextCursor":{"type":["string","null"],"minLength":1,"description":"Opaque cursor for the next page, or null when this is the final page."}}};
const schema94 = {"type":"object","additionalProperties":false,"required":["contractDigest","status","acceptedAt","activeExecutableDigest"],"properties":{"contractDigest":{"$ref":"#/$defs/Sha256Digest"},"status":{"const":"ready","description":"A version is deployed only after its generated default executable has been activated."},"acceptedAt":{"type":"string","format":"date-time"},"activeExecutableDigest":{"$ref":"#/$defs/Sha256Digest"}}};

function validate89(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate89.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.contractDigest === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contractDigest"},message:"must have required property '"+"contractDigest"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.status === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "status"},message:"must have required property '"+"status"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.acceptedAt === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "acceptedAt"},message:"must have required property '"+"acceptedAt"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.activeExecutableDigest === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "activeExecutableDigest"},message:"must have required property '"+"activeExecutableDigest"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "contractDigest") || (key0 === "status")) || (key0 === "acceptedAt")) || (key0 === "activeExecutableDigest"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.contractDigest !== undefined){
let data0 = data.contractDigest;
if(typeof data0 === "string"){
if(!pattern14.test(data0)){
const err5 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.status !== undefined){
if("ready" !== data.status){
const err7 = {instancePath:instancePath+"/status",schemaPath:"#/properties/status/const",keyword:"const",params:{allowedValue: "ready"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.acceptedAt !== undefined){
let data2 = data.acceptedAt;
if(typeof data2 === "string"){
if(!(formats0.validate(data2))){
const err8 = {instancePath:instancePath+"/acceptedAt",schemaPath:"#/properties/acceptedAt/format",keyword:"format",params:{format: "date-time"},message:"must match format \""+"date-time"+"\""};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
else {
const err9 = {instancePath:instancePath+"/acceptedAt",schemaPath:"#/properties/acceptedAt/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.activeExecutableDigest !== undefined){
let data3 = data.activeExecutableDigest;
if(typeof data3 === "string"){
if(!pattern14.test(data3)){
const err10 = {instancePath:instancePath+"/activeExecutableDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
else {
const err11 = {instancePath:instancePath+"/activeExecutableDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
}
else {
const err12 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
validate89.errors = vErrors;
return errors === 0;
}
validate89.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


function validate88(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate88.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.name === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.currentContractDigest === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "currentContractDigest"},message:"must have required property '"+"currentContractDigest"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.versions === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "versions"},message:"must have required property '"+"versions"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.nextCursor === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "nextCursor"},message:"must have required property '"+"nextCursor"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "name") || (key0 === "currentContractDigest")) || (key0 === "versions")) || (key0 === "nextCursor"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.name !== undefined){
let data0 = data.name;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err5 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(func1(data0) < 1){
const err6 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(!pattern4.test(data0)){
const err7 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/name",schemaPath:"#/$defs/DecisionName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.currentContractDigest !== undefined){
let data1 = data.currentContractDigest;
if(typeof data1 === "string"){
if(!pattern14.test(data1)){
const err9 = {instancePath:instancePath+"/currentContractDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
else {
const err10 = {instancePath:instancePath+"/currentContractDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.versions !== undefined){
let data2 = data.versions;
if(Array.isArray(data2)){
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
if(!(validate89(data2[i0], {instancePath:instancePath+"/versions/" + i0,parentData:data2,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate89.errors : vErrors.concat(validate89.errors);
errors = vErrors.length;
}
}
}
else {
const err11 = {instancePath:instancePath+"/versions",schemaPath:"#/properties/versions/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.nextCursor !== undefined){
let data4 = data.nextCursor;
if((typeof data4 !== "string") && (data4 !== null)){
const err12 = {instancePath:instancePath+"/nextCursor",schemaPath:"#/properties/nextCursor/type",keyword:"type",params:{type: schema91.properties.nextCursor.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if(typeof data4 === "string"){
if(func1(data4) < 1){
const err13 = {instancePath:instancePath+"/nextCursor",schemaPath:"#/properties/nextCursor/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
}
}
else {
const err14 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
validate88.errors = vErrors;
return errors === 0;
}
validate88.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};
