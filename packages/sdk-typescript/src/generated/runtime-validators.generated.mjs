/*
 * Generated from Flaggo v3 JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */
"use strict";
export const validateRuntimeInput = validate21;
const schema32 = {"type":"object","additionalProperties":false,"required":["attributes"],"properties":{"attributes":{"$ref":"#/$defs/RuntimeAttributes"},"currentExposure":{"$ref":"#/$defs/CurrentExposure"}}};
const schema35 = {"type":"object","additionalProperties":false,"required":["exposureId"],"properties":{"exposureId":{"type":"string","minLength":1,"maxLength":256,"description":"SDK-managed identity of the previous decision actually applied in the current activity context."}}};
const schema33 = {"type":"object","required":["_random"],"maxProperties":129,"propertyNames":{"type":"string","maxLength":128},"properties":{"_random":{"type":"number","minimum":0,"exclusiveMaximum":1,"description":"SDK-generated internal random value, reused for retries of one logical evaluation."}},"patternProperties":{"^[A-Za-z][A-Za-z0-9_]*$":{"$ref":"#/$defs/JsonValue"}},"additionalProperties":false,"description":"Complete SDK-constructed attribute map. The v3 wire profile reserves underscore-prefixed names and defines only _random. Semantic validation rejects user attributes not declared by the contract."};
const func1 = require("ajv/dist/runtime/ucs2length").default;
const pattern4 = new RegExp("^[A-Za-z][A-Za-z0-9_]*$", "u");
const schema34 = {"oneOf":[{"type":"null"},{"type":"boolean"},{"type":"number"},{"type":"string"},{"type":"array","items":{"$ref":"#/$defs/JsonValue"}},{"type":"object","additionalProperties":{"$ref":"#/$defs/JsonValue"}}]};
const wrapper0 = {validate: validate23};

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
if(!(wrapper0.validate(data[i0], {instancePath:instancePath+"/" + i0,parentData:data,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper0.validate.errors : vErrors.concat(wrapper0.validate.errors);
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
if(!(wrapper0.validate(data[key0], {instancePath:instancePath+"/" + key0.replace(/~/g, "~0").replace(/\//g, "~1"),parentData:data,parentDataProperty:key0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? wrapper0.validate.errors : vErrors.concat(wrapper0.validate.errors);
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
validate23.errors = vErrors;
evaluated0.props = props2;
evaluated0.items = items1;
return errors === 0;
}
validate23.evaluated = {"dynamicProps":true,"dynamicItems":true};


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
if(Object.keys(data).length > 129){
const err0 = {instancePath,schemaPath:"#/maxProperties",keyword:"maxProperties",params:{limit: 129},message:"must NOT have more than 129 properties"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data._random === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "_random"},message:"must have required property '"+"_random"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
const _errs1 = errors;
if(typeof key0 === "string"){
if(func1(key0) > 128){
const err2 = {instancePath,schemaPath:"#/propertyNames/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters",propertyName:key0};
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
const err3 = {instancePath,schemaPath:"#/propertyNames/type",keyword:"type",params:{type: "string"},message:"must be string",propertyName:key0};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
var valid0 = _errs1 === errors;
if(!valid0){
const err4 = {instancePath,schemaPath:"#/propertyNames",keyword:"propertyNames",params:{propertyName: key0},message:"property name must be valid"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
for(const key1 in data){
if(!((key1 === "_random") || (pattern4.test(key1)))){
const err5 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data._random !== undefined){
let data0 = data._random;
if((typeof data0 == "number") && (isFinite(data0))){
if(data0 < 0 || isNaN(data0)){
const err6 = {instancePath:instancePath+"/_random",schemaPath:"#/properties/_random/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(data0 >= 1 || isNaN(data0)){
const err7 = {instancePath:instancePath+"/_random",schemaPath:"#/properties/_random/exclusiveMaximum",keyword:"exclusiveMaximum",params:{comparison: "<", limit: 1},message:"must be < 1"};
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
const err8 = {instancePath:instancePath+"/_random",schemaPath:"#/properties/_random/type",keyword:"type",params:{type: "number"},message:"must be number"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
for(const key2 in data){
if(pattern4.test(key2)){
if(!(validate23(data[key2], {instancePath:instancePath+"/" + key2.replace(/~/g, "~0").replace(/\//g, "~1"),parentData:data,parentDataProperty:key2,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
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
validate22.errors = vErrors;
return errors === 0;
}
validate22.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


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
if(data.attributes === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "attributes"},message:"must have required property '"+"attributes"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "attributes") || (key0 === "currentExposure"))){
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
if(data.attributes !== undefined){
if(!(validate22(data.attributes, {instancePath:instancePath+"/attributes",parentData:data,parentDataProperty:"attributes",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate22.errors : vErrors.concat(validate22.errors);
errors = vErrors.length;
}
}
if(data.currentExposure !== undefined){
let data1 = data.currentExposure;
if(data1 && typeof data1 == "object" && !Array.isArray(data1)){
if(data1.exposureId === undefined){
const err2 = {instancePath:instancePath+"/currentExposure",schemaPath:"#/$defs/CurrentExposure/required",keyword:"required",params:{missingProperty: "exposureId"},message:"must have required property '"+"exposureId"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key1 in data1){
if(!(key1 === "exposureId")){
const err3 = {instancePath:instancePath+"/currentExposure",schemaPath:"#/$defs/CurrentExposure/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data1.exposureId !== undefined){
let data2 = data1.exposureId;
if(typeof data2 === "string"){
if(func1(data2) > 256){
const err4 = {instancePath:instancePath+"/currentExposure/exposureId",schemaPath:"#/$defs/CurrentExposure/properties/exposureId/maxLength",keyword:"maxLength",params:{limit: 256},message:"must NOT have more than 256 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(func1(data2) < 1){
const err5 = {instancePath:instancePath+"/currentExposure/exposureId",schemaPath:"#/$defs/CurrentExposure/properties/exposureId/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
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
const err6 = {instancePath:instancePath+"/currentExposure/exposureId",schemaPath:"#/$defs/CurrentExposure/properties/exposureId/type",keyword:"type",params:{type: "string"},message:"must be string"};
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
else {
const err7 = {instancePath:instancePath+"/currentExposure",schemaPath:"#/$defs/CurrentExposure/type",keyword:"type",params:{type: "object"},message:"must be object"};
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
validate21.errors = vErrors;
return errors === 0;
}
validate21.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

export const validateRuntimeDecision = validate26;
const schema36 = {"type":"object","additionalProperties":false,"required":["contractDigest","executableDigest","result","evaluation"],"properties":{"contractDigest":{"$ref":"#/$defs/Sha256Digest"},"executableDigest":{"$ref":"#/$defs/Sha256Digest"},"result":{"$ref":"#/$defs/JsonValue","description":"Decision result. Runtime also validates it against the accepted DecisionContract result schema."},"evaluation":{"$ref":"#/$defs/EvaluationProvenance"}}};
const schema37 = {"type":"string","pattern":"^sha256:[0-9a-f]{64}$","description":"Project digest form: sha256:<lowercase-hex>."};
const pattern6 = new RegExp("^sha256:[0-9a-f]{64}$", "u");
const schema39 = {"oneOf":[{"$ref":"#/$defs/RuleEvaluation"},{"$ref":"#/$defs/DefaultEvaluation"}]};
const schema40 = {"type":"object","additionalProperties":false,"required":["source","rule"],"properties":{"source":{"const":"rule"},"rule":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9._-]*$"}}};
const schema41 = {"type":"object","additionalProperties":false,"required":["source"],"properties":{"source":{"const":"default"}}};
const pattern8 = new RegExp("^[A-Za-z][A-Za-z0-9._-]*$", "u");

function validate28(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate28.evaluated;
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
if(data.source === undefined){
const err0 = {instancePath,schemaPath:"#/$defs/RuleEvaluation/required",keyword:"required",params:{missingProperty: "source"},message:"must have required property '"+"source"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.rule === undefined){
const err1 = {instancePath,schemaPath:"#/$defs/RuleEvaluation/required",keyword:"required",params:{missingProperty: "rule"},message:"must have required property '"+"rule"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "source") || (key0 === "rule"))){
const err2 = {instancePath,schemaPath:"#/$defs/RuleEvaluation/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.source !== undefined){
if("rule" !== data.source){
const err3 = {instancePath:instancePath+"/source",schemaPath:"#/$defs/RuleEvaluation/properties/source/const",keyword:"const",params:{allowedValue: "rule"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.rule !== undefined){
let data1 = data.rule;
if(typeof data1 === "string"){
if(func1(data1) > 128){
const err4 = {instancePath:instancePath+"/rule",schemaPath:"#/$defs/RuleEvaluation/properties/rule/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(func1(data1) < 1){
const err5 = {instancePath:instancePath+"/rule",schemaPath:"#/$defs/RuleEvaluation/properties/rule/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!pattern8.test(data1)){
const err6 = {instancePath:instancePath+"/rule",schemaPath:"#/$defs/RuleEvaluation/properties/rule/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
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
const err7 = {instancePath:instancePath+"/rule",schemaPath:"#/$defs/RuleEvaluation/properties/rule/type",keyword:"type",params:{type: "string"},message:"must be string"};
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
const err8 = {instancePath,schemaPath:"#/$defs/RuleEvaluation/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
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
if(data.source === undefined){
const err9 = {instancePath,schemaPath:"#/$defs/DefaultEvaluation/required",keyword:"required",params:{missingProperty: "source"},message:"must have required property '"+"source"+"'"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
for(const key1 in data){
if(!(key1 === "source")){
const err10 = {instancePath,schemaPath:"#/$defs/DefaultEvaluation/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.source !== undefined){
if("default" !== data.source){
const err11 = {instancePath:instancePath+"/source",schemaPath:"#/$defs/DefaultEvaluation/properties/source/const",keyword:"const",params:{allowedValue: "default"},message:"must be equal to constant"};
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
const err12 = {instancePath,schemaPath:"#/$defs/DefaultEvaluation/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
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
}
if(!valid0){
const err13 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
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
validate28.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate28.evaluated = {"dynamicProps":true,"dynamicItems":false};


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
if(data.executableDigest === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "executableDigest"},message:"must have required property '"+"executableDigest"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.result === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "result"},message:"must have required property '"+"result"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.evaluation === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "evaluation"},message:"must have required property '"+"evaluation"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "contractDigest") || (key0 === "executableDigest")) || (key0 === "result")) || (key0 === "evaluation"))){
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
if(!pattern6.test(data0)){
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
if(data.executableDigest !== undefined){
let data1 = data.executableDigest;
if(typeof data1 === "string"){
if(!pattern6.test(data1)){
const err7 = {instancePath:instancePath+"/executableDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
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
const err8 = {instancePath:instancePath+"/executableDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.result !== undefined){
if(!(validate23(data.result, {instancePath:instancePath+"/result",parentData:data,parentDataProperty:"result",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
}
if(data.evaluation !== undefined){
if(!(validate28(data.evaluation, {instancePath:instancePath+"/evaluation",parentData:data,parentDataProperty:"evaluation",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate28.errors : vErrors.concat(validate28.errors);
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
validate26.errors = vErrors;
return errors === 0;
}
validate26.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};
