mod support;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use support::{unique_temp_dir, Pty};

fn scenario(mode: &str) {
    let root = unique_temp_dir("hibiscus-loop-tty");
    let pi = root.join("pi");
    let log = root.join("loop-log");
    fs::write(&pi, r#"#!/usr/bin/env python3
import json,sys,os
args=sys.argv[1:]
flag=os.path.join(os.path.dirname(args[args.index('--extension')+1]),'loop-active')
def send(value):print(json.dumps(value,separators=(',',':')),flush=True)
rounds=0
for line in sys.stdin:
 c=json.loads(line); kind=c['type']; ident=c.get('id')
 if kind=='get_state':data={'model':{'provider':'mock','id':'test'}}
 elif kind=='get_session_stats':data={}
 elif kind=='get_messages':
  data={'messages':[{'role':'assistant','content':[{'type':'text','text':'Browser UAT finished and evidence attached.'}]}]}
  if os.environ['LOOP_MODE']=='report_file' and rounds==1:
   open(os.path.join(folder,'report.json'),'w').write(json.dumps(report))
 elif kind=='prompt':
  loop=c['message'].startswith('Hibiscus unattended /loop')
  with open(os.environ['LOOP_LOG'],'a') as f:f.write('loop='+str(loop)+' marker='+str(os.path.exists(flag))+'\n')
  send({'type':'response','id':ident,'success':True})
  if loop:
   assert "screenshot: 'on'" in c['message'] and "trace: 'retain-on-failure'" in c['message']
   rounds+=1
   if os.environ['LOOP_MODE'] in ('rate','rate_tool','weekly') and rounds==1:
    if os.environ['LOOP_MODE']=='rate_tool':send({'type':'tool_execution_start','toolCallId':'r1','toolName':'read','args':{'path':'README.md'}})
    error='quota exceeded' if os.environ['LOOP_MODE']=='weekly' else '429 temporary rate limit'
    send({'type':'message_end','message':{'role':'assistant','stopReason':'error','errorMessage':error}})
    send({'type':'agent_settled'})
    continue
   labels=['Implement and test UI']
   if os.environ['LOOP_MODE']=='missing' and rounds>1:labels.append('Verify logout')
   send({'type':'tool_execution_end','toolCallId':'goal','toolName':'goal','isError':False,'result':{'details':{'hibiscusGoal':{'completed':len(labels),'total':len(labels),'steps':[{'label':label,'done':True} for label in labels]}}}})
   if os.environ['LOOP_MODE']!='stalled' and (os.environ['LOOP_MODE'] not in ('two','evidence_mismatch') or rounds>1):
    folder=os.path.dirname(os.environ['LOOP_LOG']);screenshot=os.path.join(folder,'screenshot.png');console=os.path.join(folder,'browser-console.txt')
    open(screenshot,'wb').write(b'synthetic screenshot');open(console,'w').write('synthetic browser console')
    attachments=[{'name':'screenshot','path':screenshot},{'name':'browser-console','path':console}]
    if os.environ['LOOP_MODE'] in ('inline_first','inline_only'):
     attachments=[{'name':'screenshot','contentType':'image/png','body':'iVBORw0KGgo='},{'name':'browser-console','contentType':'text/plain','body':'bm8gZXJyb3Jz'}]
     if os.environ['LOOP_MODE']=='inline_first':attachments.append({'name':'screenshot','path':screenshot})
     else:open(screenshot,'wb').write(bytes([137,80,78,71,13,10,26,10]))
    if os.environ['LOOP_MODE']=='report_file':
     attachments=[{'name':'screenshot','path':'screenshot.png'},{'name':'browser-console','path':'browser-console.txt'}]
    specs=[{'title':label,'tests':[{'status':'expected','results':[{'status':'passed','attachments':attachments}]}]} for label in labels]
    report={'stats':{'expected':len(labels),'unexpected':0,'skipped':0,'flaky':0},'suites':[{'specs':specs}]}
    if os.environ['LOOP_MODE']=='bad_report' and rounds==1:report['stats']['expected']=0
    if os.environ['LOOP_MODE']=='report_file':
     if rounds==1:
      send({'type':'tool_execution_start','toolCallId':'e2e','toolName':'bash','args':{'command':'cd project && npx playwright test --reporter=json > report.json'}})
      send({'type':'tool_execution_end','toolCallId':'e2e','toolName':'bash','isError':False,'result':{'content':[{'type':'text','text':'output truncated'}]}})
     else:
      send({'type':'extension_ui_request','id':'evidence-bridge','method':'input','title':'Hibiscus internal loop evidence','placeholder':json.dumps({'action':'submit_evidence','projectDir':folder,'reportPath':'report.json','supportFiles':['browser-console.txt']})})
      reply=json.loads(sys.stdin.readline()); validation=json.loads(reply['value'])
      assert reply['id']=='evidence-bridge' and validation['accepted'] and validation['verified']==1, validation
      send({'type':'tool_execution_end','toolCallId':'evidence','toolName':'loop_status','isError':False,'result':{'details':{'hibiscusLoop':{'state':'submit_evidence','validatedByClient':True}}}})
    else:
     send({'type':'tool_execution_start','toolCallId':'e2e','toolName':'bash','args':{'command':'npx playwright test --reporter=json'}})
     send({'type':'tool_execution_end','toolCallId':'e2e','toolName':'bash','isError':False,'result':{'content':[{'type':'text','text':json.dumps(report)}]}})
    inspected=screenshot
    if os.environ['LOOP_MODE']=='wrong_image' and rounds==1:
     inspected=os.path.join(folder,'unrelated.png');open(inspected,'wb').write(b'unrelated synthetic image')
    send({'type':'tool_execution_start','toolCallId':'screenshot','toolName':'read','args':{'path':inspected}})
    send({'type':'tool_execution_end','toolCallId':'screenshot','toolName':'read','isError':False,'result':{'content':[{'type':'image','mimeType':'image/png','data':'aGVsbG8='}]}})
   if os.environ['LOOP_MODE'] in ('report_file','readonly'):
    command='cd project && tail -n 20 server.log && curl -I http://localhost' if os.environ['LOOP_MODE']=='report_file' else 'tail -n 20 server.log'
    send({'type':'tool_execution_start','toolCallId':'logs','toolName':'bash','args':{'command':command}})
    send({'type':'tool_execution_end','toolCallId':'logs','toolName':'bash','isError':False})
   send({'type':'tool_execution_end','toolCallId':'loopstatus','toolName':'loop_status','isError':False,'result':{'details':{'hibiscusLoop':{'state':'candidate_complete'}}}})
   send({'type':'message_end','message':{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':'Browser UAT finished and evidence attached.'}]}})
  else: send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'AFTER_LOOP_ACK'}})
  send({'type':'agent_settled'})
  continue
 else:sys.exit(13)
 send({'type':'response','id':ident,'success':True,'data':data})
"#).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).unwrap();
    let sdk = root.join("sdk.mjs");
    fs::write(&sdk, r#"export class ModelRuntime {
 static async create(){let attempts=0; return {
  getModel(provider,id){return {provider,id}},
  async completeSimple(_model,context,options){
   attempts++;
   if(process.env.LOOP_MODE==='returned_error')return {stopReason:'error',errorMessage:'PRIVATE provider details',content:[]};
   if(process.env.LOOP_MODE==='repair_verdict' && attempts===1)return {stopReason:'stop',content:[{type:'text',text:'Verdict: all checks passed'}]};
   if(context.tools?.length!==1 || context.tools[0].name!=='submit_verdict' || options.toolChoice!=='auto')throw Error('expected reporting-only tool');
   const input=JSON.parse(context.messages[0].content[0].text);
   if (input.evidence && (!input.evidenceSummary?.report?.verifiedCriteria?.length || input.evidenceSummary.report.counts.failed!==0)) throw Error('missing validated evidence packet');
   if(process.env.LOOP_MODE==='report_file' && input.evidence && !input.evidenceSummary.report.supportingEvidence?.[0]?.excerpt)throw Error('missing supporting evidence content');
   if(process.env.LOOP_MODE==='verifier_fail') throw Error('synthetic reviewer unavailable');
   if(process.env.LOOP_MODE==='invalid_verdict')return {stopReason:'stop',content:[{type:'text',text:'ordinary prose, not a verdict call'}]};
   const missing=process.env.LOOP_MODE==='missing' && input.checklist.length===1;
   const premature=process.env.LOOP_MODE==='evidence_mismatch' && !input.evidence;
   return {stopReason:'toolUse',content:[{type:'toolCall',id:'review',name:'submit_verdict',arguments:{scope:missing?'missing':'covered',verdict:missing?'not_met':premature||input.evidence?'met':'not_met',reason:missing?'Original goal requires logout UAT too':'Check browser UAT evidence',missing:missing?['Verify logout']:premature||input.evidence?[]:['Run Playwright browser UAT']}}]};
  }
 }}
}"#).unwrap();
    let mut tty = Pty::spawn(
        Command::new(env!("CARGO_BIN_EXE_hibiscus"))
            .env("HIBISCUS_PI", &pi)
            .env("LOOP_LOG", &log)
            .env("LOOP_MODE", mode)
            .env("HIBISCUS_AUTH_SDK", &sdk)
            .env("HIBISCUS_NO_UPDATE_CHECK", "1"),
    );
    tty.wait_text("Enter send");
    tty.send(b"/loop --strict implement the UI\r");
    if mode == "rate_tool" || mode == "weekly" {
        tty.wait_text(if mode == "weekly" {
            "quota exceeded"
        } else {
            "429 temporary rate limit"
        });
    } else if mode == "stalled" {
        tty.wait_text("Repeated completion claims without new work");
    } else if matches!(mode, "verifier_fail" | "returned_error" | "invalid_verdict") {
        tty.wait_text(if mode != "invalid_verdict" {
            "Loop blocked: Reviewer provider call failed"
        } else {
            "Loop blocked: Reviewer did not call submit_verdict after correction"
        });
    } else {
        tty.wait_text("✓ Loop finished");
    }
    tty.send(b"AFTER_LOOP\r");
    tty.wait_text("AFTER_LOOP_ACK");
    tty.send(b"/quit\r");
    let output = tty.finish();
    assert!(!output.contains("PRIVATE provider details"));
    assert!(
        !output.contains("HIBISCUS_LOOP_COMPLETE"),
        "internal marker must not reach the user"
    );
    if mode == "stalled" {
        assert!(
            output.contains("Repeated completion claim collapsed"),
            "duplicate text should be folded in the UI"
        );
        let shown = support::screen::snapshots(&output)
            .into_iter()
            .rev()
            .find(|view| view.contains("Repeated completion claim collapsed"))
            .unwrap();
        assert!(
            shown
                .matches("Browser UAT finished and evidence attached")
                .count()
                <= 1,
            "duplicate model summaries must be collapsed in the visible view"
        );
    }
    if !matches!(mode, "rate_tool" | "weekly") {
        assert!(
            support::screen::snapshots(&output).iter().any(|view| view
                .lines()
                .any(|line| line.contains("Loop · review") && line.contains("Esc stop"))),
            "review must keep an active loop footer"
        );
    }
    let calls = fs::read_to_string(&log).unwrap();
    let expected = if mode == "stalled" {
        format!(
            "{}loop=False marker=False\n",
            "loop=True marker=True\n".repeat(5)
        )
    } else if [
        "two",
        "rate",
        "bad_report",
        "wrong_image",
        "missing",
        "evidence_mismatch",
        "report_file",
    ]
    .contains(&mode)
    {
        "loop=True marker=True\nloop=True marker=True\nloop=False marker=False\n".to_owned()
    } else {
        "loop=True marker=True\nloop=False marker=False\n".to_owned()
    };
    assert_eq!(calls, expected);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inline_first_screenshot_and_console_do_not_hide_file_attachment() {
    scenario("inline_first");
}

#[test]
fn inline_only_screenshot_can_be_verified_by_reading_identical_image_file() {
    scenario("inline_only");
}

#[test]
fn reviewer_format_correction_does_not_repeat_pi_or_browser_work() {
    scenario("repair_verdict");
}

#[test]
fn sdk_returned_provider_error_is_not_reported_as_a_format_failure() {
    scenario("returned_error");
}

#[test]
fn saved_report_survives_wrapped_command_truncation_and_readonly_log_checks() {
    scenario("report_file");
}

#[test]
fn readonly_log_check_preserves_validated_stdout_report() {
    scenario("readonly");
}

#[test]
fn loop_waits_for_pi_settlement_and_uat_evidence_then_restores_approvals() {
    scenario("one");
}
#[test]
fn model_goal_and_completion_without_browser_uat_do_not_end_loop() {
    scenario("two");
}

#[test]
fn temporary_pre_tool_rate_limit_retries_without_resending_after_tool_effects() {
    scenario("rate");
}

#[test]
fn rate_limit_after_tool_work_stops_without_replaying() {
    scenario("rate_tool");
}

#[test]
fn quota_failure_stops_without_replaying() {
    scenario("weekly");
}

#[test]
fn passing_command_without_nonzero_uat_test_report_does_not_end_loop() {
    scenario("bad_report");
}

#[test]
fn unrelated_image_read_does_not_satisfy_uat_screenshot() {
    scenario("wrong_image");
}

#[test]
fn independent_reviewer_can_expand_missing_original_scope_before_locking() {
    scenario("missing");
}

#[test]
fn unavailable_reviewer_blocks_completion_and_restores_approvals() {
    scenario("verifier_fail");
}

#[test]
fn malformed_reviewer_verdict_reports_specific_blocker() {
    scenario("invalid_verdict");
}

#[test]
fn reviewer_met_without_uat_evidence_requests_more_work() {
    scenario("evidence_mismatch");
}

#[test]
fn repeated_completion_claims_collapse_then_block_after_targeted_recovery() {
    scenario("stalled");
}
