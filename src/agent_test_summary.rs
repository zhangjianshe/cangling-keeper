//! Conservative extraction from untrusted command output, not goal acceptance.
use serde_json::{json,Value};
pub(crate) fn summarize(stdout:&str,stderr:&str,exit:Option<i32>,incomplete:bool)->Value{
 let text=format!("{stdout}\n{stderr}");let lines:Vec<_>=text.lines().map(str::trim).collect();
 let runs:Vec<_>=lines.iter().filter_map(|line|{let parts:Vec<_>=line.split_whitespace().collect();if parts.len()>=5&&parts[0]=="Ran"&&matches!(parts[2],"test"|"tests")&&parts[3]=="in"{parts[1].parse::<u64>().ok()}else{None}}).collect();
 let endings:Vec<_>=lines.iter().copied().filter(|s|*s=="OK"||s.starts_with("OK (")||s.starts_with("FAILED (")).collect();
 let failures:Vec<_>=lines.iter().filter(|s|s.starts_with("FAIL: ")||s.starts_with("ERROR: ")).take(8).map(|s|s.chars().take(120).collect::<String>()).collect();
 let recognized=runs.len()==1&&endings.len()==1;
 let ending=endings.last().copied().unwrap_or("");
 let count=|key:&str|->u64{ending.split(&format!("{key}=")).nth(1).and_then(|s|s.split(|c:char|!c.is_ascii_digit()).next()).and_then(|s|s.parse().ok()).unwrap_or(0)};
 let verdict=if recognized && (ending.starts_with("FAILED")||exit.is_some_and(|n|n!=0)){"failed"}else if !recognized||incomplete||exit!=Some(0){"unknown"}else if runs[0]==0{"no_tests"}else if count("skipped")>0||count("expected failures")>0{"passed_with_exclusions"}else{"passed"};
 json!({"format":if recognized{"unittest"}else{"unknown"},"verdict":verdict,"total":if recognized{json!(runs[0])}else{Value::Null},"failures":if recognized{json!(count("failures"))}else{Value::Null},"errors":if recognized{json!(count("errors"))}else{Value::Null},"skipped":if recognized{json!(count("skipped"))}else{Value::Null},"failureCases":failures,"diagnostics":diagnostics(&text),"incompleteOutput":incomplete,"source":"untrusted_command_output","scope":"本次命令报告，不代表需求全部验收；子集、伪造日志及脚本吞掉失败须另行核实"})
}
#[cfg(test)] mod tests{
 use super::*;
 #[test]fn captures_failures_even_when_shell_returns_zero(){let v=summarize("","FAIL: test_bad\nRan 3 tests in 0.1s\nFAILED (failures=1, errors=1)",Some(0),false);assert_eq!(v["verdict"],"failed");assert_eq!(v["errors"],1);}
 #[test]fn does_not_infer_acceptance_from_ok_or_zero(){for s in ["OK","", "Ran 0 tests in 0s\nOK"]{assert_ne!(summarize(s,"",Some(0),false)["verdict"],"passed");}assert_eq!(summarize("Ran 2 tests in 0s\nOK","",Some(0),true)["verdict"],"unknown");}
 #[test]fn excludes_skips_and_multiple_suites(){assert_eq!(summarize("Ran 2 tests in 1s\nOK (skipped=1)","",Some(0),false)["verdict"],"passed_with_exclusions");assert_eq!(summarize("Ran 2 tests in 1s\nOK\nRan 3 tests in 1s\nOK","",Some(0),false)["verdict"],"unknown");assert_eq!(summarize("Ran 2 tests in 1s\nOK","",Some(0),false)["total"],2);}
}

/// Only canonical plain unittest discovery commands share a test identity.
/// Shell pipelines and arbitrary scripts remain separate, not guessed equivalent.
pub(crate) fn identity(command:&str)->Option<String>{
 let words:Vec<_>=command.split_whitespace().collect();
 if words.len()<4||!matches!(words[0],"python"|"python3")||words[1..4]!=["-m","unittest","discover"]{return None;}
 let(mut directory,mut pattern)=(".","test*.py");let mut i=4;
 while i<words.len(){match words[i]{"-v"|"-q"=>i+=1,"-s"|"-p"=>{let field=words[i];i+=1;let v=words.get(i)?.trim_matches('"');if v.is_empty()||!v.bytes().all(|b|b.is_ascii_alphanumeric()||b"_./*?-".contains(&b))||v.split('/').any(|p|p==".."){return None;}if field=="-s"{directory=v}else{pattern=v};i+=1;},_=>return None}}
 Some(format!("unittest:{directory}:{pattern}"))
}

/// Bounded observations extracted from Python tracebacks, never executable advice.
/// Keep distinct exception locations; repeated HTTP/test traces share one entry.
fn diagnostics(text: &str) -> Vec<Value> {
    let mut found: Vec<Value> = Vec::new();
    let mut frames: Vec<String> = Vec::new();
    let mut source = String::new();
    let mut case = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("FAIL: ") || line.starts_with("ERROR: ") { case = line.chars().take(240).collect(); }
        if line.starts_with("Traceback (most recent call last)") { frames.clear(); source.clear(); continue; }
        if line.starts_with("File \"") && line.contains("\", line ") {
            if frames.len() == 3 { frames.remove(0); }
            frames.push(line.chars().take(300).collect());
            source.clear();
            continue;
        }
        if !frames.is_empty() && raw.starts_with("    ") && !line.is_empty() { source = line.chars().take(400).collect(); }
        let Some((kind, message)) = line.split_once(':') else { continue; };
        if kind.is_empty() || kind.len() > 100 || !kind.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_' || c==b'.') { continue; }
        if !(kind.ends_with("Error") || kind.ends_with("Exception") || kind=="StopIteration") { continue; }
        let message: String = message.trim().chars().take(400).collect();
        let location = frames.last().cloned().unwrap_or_default();
        if let Some(previous) = found.iter_mut().find(|v|v["exception"]==kind && v["message"]==message && v["location"]==location) {
            previous["occurrences"] = json!(previous["occurrences"].as_u64().unwrap_or(1)+1);
        } else if found.len() < 8 {
            found.push(json!({"exception":kind,"message":message,"location":location,"frames":frames,"sourceLine":source,"testCase":case,"occurrences":1}));
        }
        frames.clear();
    }
    found
}
#[cfg(test)] mod diagnostic_tests {
 use super::*;
 #[test] fn groups_repeated_tracebacks_without_losing_location() {
  let trace="Traceback (most recent call last):\n  File \"/project/service.py\", line 33, in connect\n    connect(path)\nTypeError: path is None\n";
  let v=summarize("",&format!("{}{}Ran 2 tests in 1s\nFAILED (errors=2)",trace,trace),Some(1),false);
  assert_eq!(v["diagnostics"].as_array().unwrap().len(),1);
  assert_eq!(v["diagnostics"][0]["occurrences"],2);
  assert!(v["diagnostics"][0]["location"].as_str().unwrap().contains("service.py"));
  assert_eq!(v["verdict"],"failed");
 }
 #[test] fn diagnostics_are_bounded_and_do_not_certify_tests() {
  let text=(0..30).map(|i|format!("ValueError: {}{}\n",i,"中".repeat(1000))).collect::<String>();
  let v=summarize(&text,"",Some(0),false);
  assert_eq!(v["diagnostics"].as_array().unwrap().len(),8);
  assert!(v["diagnostics"][0]["message"].as_str().unwrap().chars().count()<=400);
  assert_eq!(v["verdict"],"unknown");
  assert!(diagnostics("ERROR: test_bad\nordinary text").is_empty());
 }
}

/// Narrow quality guard, not a shell security parser. Log viewing is unaffected.
pub(crate) fn truncates_test_output(command:&str)->bool {
 let mut pipeline=command.split('|');let first=pipeline.next().unwrap_or("");
 let invocation=first.rsplit("&&").next().unwrap_or("").trim();
 let words:Vec<_>=invocation.split_whitespace().collect();
 words.len()>=3 && matches!(words[0],"python"|"python3") && words[1..3]==["-m","unittest"] && pipeline.any(|part|matches!(part.split_whitespace().next(),Some("head"|"tail")))
}
#[cfg(test)] mod truncation_tests {
 use super::*;
 #[test] fn detects_test_pipeline_but_not_log_viewing(){
  for s in ["python3 -m unittest discover -s tests -v 2>&1 | head -100", "cd /tmp/project && python -m unittest | tail -20"] {assert!(truncates_test_output(s));}
  for s in ["head -20 keeper-test.log","cat keeper-test.log | tail -20","echo python3 -m unittest | head -1","python3 -m unittest discover -s tests -v"]{assert!(!truncates_test_output(s));}
 }
}

#[cfg(test)] mod repair_context_tests {
 use super::*;
 #[test] fn includes_actual_assertion_and_case_without_inventing_cause() {
  let s="FAIL: test_count (Suite)\nTraceback (most recent call last):\n  File \"tests/check.py\", line 42, in test_count\n    self.assertEqual(body['total'], 1)\nAssertionError: 8 != 1\nRan 1 test in 0.1s\nFAILED (failures=1)";
  let v=summarize("",s,Some(1),false);
  assert_eq!(v["diagnostics"][0]["sourceLine"],"self.assertEqual(body['total'], 1)");
  assert_eq!(v["diagnostics"][0]["testCase"],"FAIL: test_count (Suite)");
 }
}
