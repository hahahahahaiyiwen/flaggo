/*
 * Generated from Flaggo JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */
"use strict";
export const validateDeploymentManifest = validate32;
const schema32 = {"type":"object","additionalProperties":false,"required":["format","authority","contracts"],"properties":{"format":{"const":"flaggo.deploy/v2"},"authority":{"$ref":"#/$defs/AuthorityScope"},"contracts":{"type":"array","items":{"$ref":"#/$defs/DecisionContractPath"},"minItems":1,"maxItems":128,"uniqueItems":true,"description":"Authority-free authored DecisionContracts that deployment binds to the manifest authority before validation, digesting, and persistence."}}};
const schema37 = {"type":"string","minLength":1,"maxLength":1024,"pattern":"^(?!.*(?:^|/)(?:[Cc][Oo][Nn]|[Pp][Rr][Nn]|[Aa][Uu][Xx]|[Nn][Uu][Ll]|[Cc][Oo][Mm][1-9]|[Ll][Pp][Tt][1-9])(?:\\.[^/]*)?(?:/|$))(?:(?:[A-Za-z0-9_-]|[A-Za-z0-9._-]*[A-Za-z0-9_-])/)*[A-Za-z][A-Za-z0-9._-]*\\.decision-contract\\.json$","description":"Portable ASCII forward-slash relative path contained by the deployment manifest directory."};
const schema33 = {"type":"object","additionalProperties":false,"required":["tenant","application","environment"],"properties":{"tenant":{"$ref":"#/$defs/AuthorityIdentifier"},"application":{"$ref":"#/$defs/AuthorityIdentifier"},"environment":{"$ref":"#/$defs/AuthorityIdentifier"}},"description":"Declared routing authority injected into every deployed DecisionContract and application telemetry signal in one deployment."};
const schema34 = {"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9._-]*$"};
const func1 = require("ajv/dist/runtime/ucs2length").default;
const pattern4 = new RegExp("^[A-Za-z][A-Za-z0-9._-]*$", "u");

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
if(data.tenant === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "tenant"},message:"must have required property '"+"tenant"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.application === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "application"},message:"must have required property '"+"application"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.environment === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "environment"},message:"must have required property '"+"environment"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "tenant") || (key0 === "application")) || (key0 === "environment"))){
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
if(data.tenant !== undefined){
let data0 = data.tenant;
if(typeof data0 === "string"){
if(func1(data0) > 128){
const err4 = {instancePath:instancePath+"/tenant",schemaPath:"#/$defs/AuthorityIdentifier/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(func1(data0) < 1){
const err5 = {instancePath:instancePath+"/tenant",schemaPath:"#/$defs/AuthorityIdentifier/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!pattern4.test(data0)){
const err6 = {instancePath:instancePath+"/tenant",schemaPath:"#/$defs/AuthorityIdentifier/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
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
const err7 = {instancePath:instancePath+"/tenant",schemaPath:"#/$defs/AuthorityIdentifier/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.application !== undefined){
let data1 = data.application;
if(typeof data1 === "string"){
if(func1(data1) > 128){
const err8 = {instancePath:instancePath+"/application",schemaPath:"#/$defs/AuthorityIdentifier/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(func1(data1) < 1){
const err9 = {instancePath:instancePath+"/application",schemaPath:"#/$defs/AuthorityIdentifier/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(!pattern4.test(data1)){
const err10 = {instancePath:instancePath+"/application",schemaPath:"#/$defs/AuthorityIdentifier/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
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
const err11 = {instancePath:instancePath+"/application",schemaPath:"#/$defs/AuthorityIdentifier/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.environment !== undefined){
let data2 = data.environment;
if(typeof data2 === "string"){
if(func1(data2) > 128){
const err12 = {instancePath:instancePath+"/environment",schemaPath:"#/$defs/AuthorityIdentifier/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if(func1(data2) < 1){
const err13 = {instancePath:instancePath+"/environment",schemaPath:"#/$defs/AuthorityIdentifier/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(!pattern4.test(data2)){
const err14 = {instancePath:instancePath+"/environment",schemaPath:"#/$defs/AuthorityIdentifier/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\""};
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
const err15 = {instancePath:instancePath+"/environment",schemaPath:"#/$defs/AuthorityIdentifier/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
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
validate22.errors = vErrors;
return errors === 0;
}
validate22.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const func0 = require("ajv/dist/runtime/equal").default;
const pattern7 = new RegExp("^(?!.*(?:^|/)(?:[Cc][Oo][Nn]|[Pp][Rr][Nn]|[Aa][Uu][Xx]|[Nn][Uu][Ll]|[Cc][Oo][Mm][1-9]|[Ll][Pp][Tt][1-9])(?:\\.[^/]*)?(?:/|$))(?:(?:[A-Za-z0-9_-]|[A-Za-z0-9._-]*[A-Za-z0-9_-])/)*[A-Za-z][A-Za-z0-9._-]*\\.decision-contract\\.json$", "u");

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
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.format === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "format"},message:"must have required property '"+"format"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.authority === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "authority"},message:"must have required property '"+"authority"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.contracts === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contracts"},message:"must have required property '"+"contracts"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "format") || (key0 === "authority")) || (key0 === "contracts"))){
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
if(data.format !== undefined){
if("flaggo.deploy/v2" !== data.format){
const err4 = {instancePath:instancePath+"/format",schemaPath:"#/properties/format/const",keyword:"const",params:{allowedValue: "flaggo.deploy/v2"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.authority !== undefined){
if(!(validate22(data.authority, {instancePath:instancePath+"/authority",parentData:data,parentDataProperty:"authority",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate22.errors : vErrors.concat(validate22.errors);
errors = vErrors.length;
}
}
if(data.contracts !== undefined){
let data2 = data.contracts;
if(Array.isArray(data2)){
if(data2.length > 128){
const err5 = {instancePath:instancePath+"/contracts",schemaPath:"#/properties/contracts/maxItems",keyword:"maxItems",params:{limit: 128},message:"must NOT have more than 128 items"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data2.length < 1){
const err6 = {instancePath:instancePath+"/contracts",schemaPath:"#/properties/contracts/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
let data3 = data2[i0];
if(typeof data3 === "string"){
if(func1(data3) > 1024){
const err7 = {instancePath:instancePath+"/contracts/" + i0,schemaPath:"#/$defs/DecisionContractPath/maxLength",keyword:"maxLength",params:{limit: 1024},message:"must NOT have more than 1024 characters"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(func1(data3) < 1){
const err8 = {instancePath:instancePath+"/contracts/" + i0,schemaPath:"#/$defs/DecisionContractPath/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!pattern7.test(data3)){
const err9 = {instancePath:instancePath+"/contracts/" + i0,schemaPath:"#/$defs/DecisionContractPath/pattern",keyword:"pattern",params:{pattern: "^(?!.*(?:^|/)(?:[Cc][Oo][Nn]|[Pp][Rr][Nn]|[Aa][Uu][Xx]|[Nn][Uu][Ll]|[Cc][Oo][Mm][1-9]|[Ll][Pp][Tt][1-9])(?:\\.[^/]*)?(?:/|$))(?:(?:[A-Za-z0-9_-]|[A-Za-z0-9._-]*[A-Za-z0-9_-])/)*[A-Za-z][A-Za-z0-9._-]*\\.decision-contract\\.json$"},message:"must match pattern \""+"^(?!.*(?:^|/)(?:[Cc][Oo][Nn]|[Pp][Rr][Nn]|[Aa][Uu][Xx]|[Nn][Uu][Ll]|[Cc][Oo][Mm][1-9]|[Ll][Pp][Tt][1-9])(?:\\.[^/]*)?(?:/|$))(?:(?:[A-Za-z0-9_-]|[A-Za-z0-9._-]*[A-Za-z0-9_-])/)*[A-Za-z][A-Za-z0-9._-]*\\.decision-contract\\.json$"+"\""};
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
const err10 = {instancePath:instancePath+"/contracts/" + i0,schemaPath:"#/$defs/DecisionContractPath/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
let i1 = data2.length;
let j0;
if(i1 > 1){
outer0:
for(;i1--;){
for(j0 = i1; j0--;){
if(func0(data2[i1], data2[j0])){
const err11 = {instancePath:instancePath+"/contracts",schemaPath:"#/properties/contracts/uniqueItems",keyword:"uniqueItems",params:{i: i1, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i1+" are identical)"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
break outer0;
}
}
}
}
}
else {
const err12 = {instancePath:instancePath+"/contracts",schemaPath:"#/properties/contracts/type",keyword:"type",params:{type: "array"},message:"must be array"};
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
const err13 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
validate32.errors = vErrors;
return errors === 0;
}
validate32.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

export const validateServiceEndpoints = validate34;
const schema39 = {"type":"object","additionalProperties":false,"required":["contractServiceUrl","decisionServiceUrl","otlpIngestionUrl"],"properties":{"contractServiceUrl":{"$ref":"#/$defs/ServiceUrl"},"decisionServiceUrl":{"$ref":"#/$defs/ServiceUrl"},"otlpIngestionUrl":{"$ref":"#/$defs/ServiceUrl"}}};
const schema40 = {"type":"string","format":"uri","maxLength":2048,"pattern":"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$","description":"Absolute HTTP(S) service base URL with an optional port from 1 through 65535 and without credentials, query, or fragment."};
const formats0 = require("ajv-formats/dist/formats").fullFormats.uri;
const pattern8 = new RegExp("^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$", "u");

function validate34(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate34.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.contractServiceUrl === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contractServiceUrl"},message:"must have required property '"+"contractServiceUrl"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.decisionServiceUrl === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "decisionServiceUrl"},message:"must have required property '"+"decisionServiceUrl"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.otlpIngestionUrl === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "otlpIngestionUrl"},message:"must have required property '"+"otlpIngestionUrl"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "contractServiceUrl") || (key0 === "decisionServiceUrl")) || (key0 === "otlpIngestionUrl"))){
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
if(data.contractServiceUrl !== undefined){
let data0 = data.contractServiceUrl;
if(typeof data0 === "string"){
if(func1(data0) > 2048){
const err4 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!pattern8.test(data0)){
const err5 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!(formats0(data0))){
const err6 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err7 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.decisionServiceUrl !== undefined){
let data1 = data.decisionServiceUrl;
if(typeof data1 === "string"){
if(func1(data1) > 2048){
const err8 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!pattern8.test(data1)){
const err9 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(!(formats0(data1))){
const err10 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err11 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.otlpIngestionUrl !== undefined){
let data2 = data.otlpIngestionUrl;
if(typeof data2 === "string"){
if(func1(data2) > 2048){
const err12 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if(!pattern8.test(data2)){
const err13 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(!(formats0(data2))){
const err14 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err15 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
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
validate34.errors = vErrors;
return errors === 0;
}
validate34.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

export const validateRuntimeConfiguration = validate35;
const schema38 = {"type":"object","additionalProperties":false,"required":["format","authority","services","bindings"],"properties":{"format":{"const":"flaggo.runtime-config/v1"},"authority":{"$ref":"#/$defs/AuthorityScope"},"services":{"$ref":"#/$defs/ServiceEndpoints"},"bindings":{"type":"object","propertyNames":{"$ref":"#/$defs/DecisionName"},"additionalProperties":{"$ref":"#/$defs/ContractBinding"},"minProperties":1,"maxProperties":128}},"description":"Generated immutable application configuration containing the same authority bound into every deployed DecisionContract."};
const schema43 = {"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9._-]*$"};

function validate27(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate27.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.contractServiceUrl === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "contractServiceUrl"},message:"must have required property '"+"contractServiceUrl"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.decisionServiceUrl === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "decisionServiceUrl"},message:"must have required property '"+"decisionServiceUrl"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.otlpIngestionUrl === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "otlpIngestionUrl"},message:"must have required property '"+"otlpIngestionUrl"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "contractServiceUrl") || (key0 === "decisionServiceUrl")) || (key0 === "otlpIngestionUrl"))){
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
if(data.contractServiceUrl !== undefined){
let data0 = data.contractServiceUrl;
if(typeof data0 === "string"){
if(func1(data0) > 2048){
const err4 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!pattern8.test(data0)){
const err5 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!(formats0(data0))){
const err6 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err7 = {instancePath:instancePath+"/contractServiceUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.decisionServiceUrl !== undefined){
let data1 = data.decisionServiceUrl;
if(typeof data1 === "string"){
if(func1(data1) > 2048){
const err8 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!pattern8.test(data1)){
const err9 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(!(formats0(data1))){
const err10 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err11 = {instancePath:instancePath+"/decisionServiceUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.otlpIngestionUrl !== undefined){
let data2 = data.otlpIngestionUrl;
if(typeof data2 === "string"){
if(func1(data2) > 2048){
const err12 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/maxLength",keyword:"maxLength",params:{limit: 2048},message:"must NOT have more than 2048 characters"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if(!pattern8.test(data2)){
const err13 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/pattern",keyword:"pattern",params:{pattern: "^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"},message:"must match pattern \""+"^https?://(?:\\[[0-9A-Fa-f:.]+\\]|[^\\s/:?#@]+)(?::(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:/[^\\s?#]*)?$"+"\""};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(!(formats0(data2))){
const err14 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/format",keyword:"format",params:{format: "uri"},message:"must match format \""+"uri"+"\""};
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
const err15 = {instancePath:instancePath+"/otlpIngestionUrl",schemaPath:"#/$defs/ServiceUrl/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
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
validate27.errors = vErrors;
return errors === 0;
}
validate27.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema44 = {"type":"object","additionalProperties":false,"required":["contractDigest"],"properties":{"contractDigest":{"$ref":"#/$defs/Sha256Digest"}}};
const schema45 = {"type":"string","pattern":"^sha256:[0-9a-f]{64}$"};
const pattern12 = new RegExp("^sha256:[0-9a-f]{64}$", "u");

function validate29(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate29.evaluated;
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
for(const key0 in data){
if(!(key0 === "contractDigest")){
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
if(data.contractDigest !== undefined){
let data0 = data.contractDigest;
if(typeof data0 === "string"){
if(!pattern12.test(data0)){
const err2 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/pattern",keyword:"pattern",params:{pattern: "^sha256:[0-9a-f]{64}$"},message:"must match pattern \""+"^sha256:[0-9a-f]{64}$"+"\""};
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
const err3 = {instancePath:instancePath+"/contractDigest",schemaPath:"#/$defs/Sha256Digest/type",keyword:"type",params:{type: "string"},message:"must be string"};
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
validate29.errors = vErrors;
return errors === 0;
}
validate29.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


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
if(data.format === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "format"},message:"must have required property '"+"format"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.authority === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "authority"},message:"must have required property '"+"authority"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.services === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "services"},message:"must have required property '"+"services"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.bindings === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "bindings"},message:"must have required property '"+"bindings"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "format") || (key0 === "authority")) || (key0 === "services")) || (key0 === "bindings"))){
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
if(data.format !== undefined){
if("flaggo.runtime-config/v1" !== data.format){
const err5 = {instancePath:instancePath+"/format",schemaPath:"#/properties/format/const",keyword:"const",params:{allowedValue: "flaggo.runtime-config/v1"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.authority !== undefined){
if(!(validate22(data.authority, {instancePath:instancePath+"/authority",parentData:data,parentDataProperty:"authority",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate22.errors : vErrors.concat(validate22.errors);
errors = vErrors.length;
}
}
if(data.services !== undefined){
if(!(validate27(data.services, {instancePath:instancePath+"/services",parentData:data,parentDataProperty:"services",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate27.errors : vErrors.concat(validate27.errors);
errors = vErrors.length;
}
}
if(data.bindings !== undefined){
let data3 = data.bindings;
if(data3 && typeof data3 == "object" && !Array.isArray(data3)){
if(Object.keys(data3).length > 128){
const err6 = {instancePath:instancePath+"/bindings",schemaPath:"#/properties/bindings/maxProperties",keyword:"maxProperties",params:{limit: 128},message:"must NOT have more than 128 properties"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(Object.keys(data3).length < 1){
const err7 = {instancePath:instancePath+"/bindings",schemaPath:"#/properties/bindings/minProperties",keyword:"minProperties",params:{limit: 1},message:"must NOT have fewer than 1 properties"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
for(const key1 in data3){
const _errs7 = errors;
if(typeof key1 === "string"){
if(func1(key1) > 128){
const err8 = {instancePath:instancePath+"/bindings",schemaPath:"#/$defs/DecisionName/maxLength",keyword:"maxLength",params:{limit: 128},message:"must NOT have more than 128 characters",propertyName:key1};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(func1(key1) < 1){
const err9 = {instancePath:instancePath+"/bindings",schemaPath:"#/$defs/DecisionName/minLength",keyword:"minLength",params:{limit: 1},message:"must NOT have fewer than 1 characters",propertyName:key1};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(!pattern4.test(key1)){
const err10 = {instancePath:instancePath+"/bindings",schemaPath:"#/$defs/DecisionName/pattern",keyword:"pattern",params:{pattern: "^[A-Za-z][A-Za-z0-9._-]*$"},message:"must match pattern \""+"^[A-Za-z][A-Za-z0-9._-]*$"+"\"",propertyName:key1};
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
const err11 = {instancePath:instancePath+"/bindings",schemaPath:"#/$defs/DecisionName/type",keyword:"type",params:{type: "string"},message:"must be string",propertyName:key1};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
var valid1 = _errs7 === errors;
if(!valid1){
const err12 = {instancePath:instancePath+"/bindings",schemaPath:"#/properties/bindings/propertyNames",keyword:"propertyNames",params:{propertyName: key1},message:"property name must be valid"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
for(const key2 in data3){
if(!(validate29(data3[key2], {instancePath:instancePath+"/bindings/" + key2.replace(/~/g, "~0").replace(/\//g, "~1"),parentData:data3,parentDataProperty:key2,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate29.errors : vErrors.concat(validate29.errors);
errors = vErrors.length;
}
}
}
else {
const err13 = {instancePath:instancePath+"/bindings",schemaPath:"#/properties/bindings/type",keyword:"type",params:{type: "object"},message:"must be object"};
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
const err14 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
validate35.errors = vErrors;
return errors === 0;
}
validate35.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};
