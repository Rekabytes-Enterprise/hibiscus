// Exercise the embedded helper in an isolated process with a fake Pi SDK.
// No provider calls, user settings, credentials or browser artifacts are used.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(tmpdir(), 'hibiscus-loop-verdict-'));
try {
  const sdk = join(root, 'sdk.mjs');
  const calls = join(root, 'calls.jsonl');
  writeFileSync(sdk, `
import { appendFileSync } from 'node:fs';
let count = 0;
export class ModelRuntime {
  static async create() {
    const mode = process.env.VERIFIER_CASE;
    if (mode === 'init_error') throw Error('PRIVATE_INIT_ERROR');
    if (mode === 'old_sdk') return {};
    return {
      getModel(provider, id) { return mode === 'no_model' ? undefined : {provider, id, compat:{supportsStrictMode:false}}; },
      async completeSimple(model, context, options) {
        count++;
        appendFileSync(process.env.VERIFIER_CALLS, JSON.stringify({ model, context,
          options:{toolChoice:options.toolChoice,cacheRetention:options.cacheRetention},count })+'\\n');
        const good = {scope:'covered', verdict:'met', reason:'All criteria supported', missing:[]};
        const call = args => ({type:'toolCall', id:'verdict-'+count, name:'submit_verdict', arguments:args});
        const reply = args => ({stopReason:'toolUse', content:[call(args)]});
        if (mode === 'throw_error') throw Error('PRIVATE_PROVIDER_ERROR');
        if (['error','aborted','length','deferred','pending'].includes(mode))
          return {stopReason:mode,errorMessage:'PRIVATE_PROVIDER_ERROR',content:[call(good)]};
        if (mode === 'no_stop') return {content:[call(good)]};
        if (mode === 'repair' && count === 1) return {stopReason:'stop',content:[{type:'text',text:'PRIVATE prose'}]};
        if (mode === 'repair_args' && count === 1) return reply({...good,missing:'wrong type'});
        if (mode === 'text_json') return {stopReason:'stop',content:[{type:'text',text:JSON.stringify(good)}]};
        if (mode === 'multiple') return {stopReason:'toolUse',content:[call(good),call(good)]};
        if (mode === 'wrong_tool') return {stopReason:'toolUse',content:[{...call(good),name:'bash'}]};
        if (mode === 'string_args') return reply(JSON.stringify(good));
        if (mode === 'missing_field') {delete good.missing;return reply(good);}
        if (mode === 'extra_field') return reply({...good,extra:true});
        if (mode === 'too_long') return reply({...good,reason:'x'.repeat(501)});
        if (mode === 'contradictory') return reply({...good,scope:'missing'});
        if (mode === 'oversized_wire') return reply({scope:'missing',verdict:'not_met',reason:'Need fixes',missing:Array(50).fill('界'.repeat(200))});
        if (mode === 'missing_scope') return reply({scope:'missing',verdict:'not_met',reason:'Logout not tested',missing:['Logout flow']});
        if (mode === 'blocked') return reply({scope:'covered',verdict:'blocked',reason:'Required service inaccessible',missing:['Service access']});
        if (mode === 'text_and_tool') return {stopReason:'toolUse',content:[{type:'text',text:'PRIVATE prose'},call(good)]};
        return reply(good);
      }
    };
  }
}
`);
  const helper = readFileSync(new URL('../src/pi/loop-verify.mjs', import.meta.url), 'utf8');
  const request = {goal:'Test sign in and sign out',steps:['Sign in','Sign out'], evidence:true,
    evidenceSummary:{verifiedCriteria:['Sign in','Sign out']}, report:'Tests ran', provider:'fixture', modelId:'fixture'};
  function run(mode, expected, expectedCalls) {
    rmSync(calls, { force:true });
    const result = spawnSync(process.execPath, ['--input-type=module','--eval',helper,'--',sdk], {
      input: JSON.stringify(request)+'\n', encoding:'utf8', timeout:5000,
      env:{...process.env, VERIFIER_CASE:mode, VERIFIER_CALLS:calls},
    });
    assert.equal(result.error, undefined, mode);
    assert.equal(result.status, 0, `${mode}: ${result.stderr}`);
    const value = JSON.parse(result.stdout.trim());
    assert.equal(value.code ?? value.verdict, expected, mode);
    assert.ok(!result.stdout.includes('PRIVATE'), 'raw provider errors/prose must not escape');
    const records = expectedCalls ? readFileSync(calls,'utf8').trim().split('\n').map(JSON.parse) : [];
    assert.equal(records.length, expectedCalls, mode);
    for (const {context,options} of records) {
      assert.deepEqual(context.tools.map(tool => tool.name), ['submit_verdict']);
      assert.equal(context.tools[0].parameters.additionalProperties,false);
      assert.deepEqual(context.tools[0].constrainedSampling,{type:'json_schema',strict:'prefer'});
      assert.equal(context.tools[0].execute,undefined, 'report tool has no executor');
      assert.equal(options.toolChoice,'auto');
      const input=JSON.parse(context.messages[0].content[0].text);
      assert.equal(input.originalGoal,request.goal);
      assert.deepEqual(input.evidenceSummary,request.evidenceSummary);
    }
    if(expectedCalls===2) {
      assert.deepEqual(records[0].context.messages[0],records[1].context.messages[0], 'correction retains the original evidence');
      assert.equal(records[1].context.messages.length,2);
    }
  }
  for(const mode of ['valid','text_and_tool']) run(mode,'met',1);
  for(const mode of ['repair','repair_args']) run(mode,'met',2);
  run('missing_scope','not_met',1);
  run('blocked','blocked',1);
  run('text_json','verdict_tool_missing',2);
  for(const mode of ['multiple','wrong_tool']) run(mode,'verdict_tool_unexpected',2);
  for(const mode of ['string_args','missing_field','extra_field','too_long','contradictory','oversized_wire']) run(mode,'invalid_verdict_arguments',2);
  for(const [mode, code] of Object.entries({error:'model_call_failed',aborted:'model_call_aborted',length:'model_output_truncated',deferred:'model_response_deferred',pending:'invalid_response',no_stop:'invalid_response',throw_error:'model_call_failed'})) run(mode,code,1);
  run('old_sdk','sdk_api_unavailable',0);
  run('no_model','model_unavailable',0);
  run('init_error','runtime_init_failed',0);
} finally {
  rmSync(root, { recursive:true, force:true });
}
console.log('loop reviewer native tool verdict tests passed');
