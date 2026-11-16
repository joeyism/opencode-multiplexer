use std::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{Error as DeError, MapAccess, Visitor},
};

pub fn parse_playbook(yaml: &str) -> anyhow::Result<Playbook> {
    Ok(serde_yaml::from_str(yaml)?)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Playbook {
    pub name: String,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_params")]
    pub params: Vec<(String, Param)>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Param {
    pub prompt: String,
    #[serde(default = "default_true")]
    pub required: bool,
    pub default: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Send {
        send: SendSpec,
    },
    Run {
        run: String,
        register: Option<String>,
    },
    Until {
        until: String,
        max: u32,
        steps: Vec<Step>,
    },
    If {
        cond: String,
        then: Vec<Step>,
        else_steps: Vec<Step>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SendSpec {
    pub agent: String,
    pub message: String,
    pub timeout_secs: Option<u64>,
}

fn deserialize_params<'de, D>(deserializer: D) -> Result<Vec<(String, Param)>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ParamsVisitor;

    impl<'de> Visitor<'de> for ParamsVisitor {
        type Value = Vec<(String, Param)>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a mapping of parameter names to parameter specs")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some((k, v)) = map.next_entry::<String, Param>()? {
                out.push((k, v));
            }
            Ok(out)
        }
    }

    deserializer.deserialize_map(ParamsVisitor)
}

fn map_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(serde_yaml::Value::String(key.to_string()))
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        let map = value
            .as_mapping()
            .ok_or_else(|| D::Error::custom("step must be a mapping with send/run/until/if"))?;

        if let Some(send_val) = map_get(map, "send") {
            let send: SendSpec = serde_yaml::from_value(send_val.clone())
                .map_err(|e| D::Error::custom(format!("send: {e}")))?;
            return Ok(Step::Send { send });
        }

        if map_get(map, "until").is_some() {
            let until: String =
                serde_yaml::from_value(map_get(map, "until").expect("checked").clone())
                    .map_err(|e| D::Error::custom(format!("until: {e}")))?;

            let max: u32 = match map_get(map, "max") {
                Some(v) => serde_yaml::from_value(v.clone())
                    .map_err(|e| D::Error::custom(format!("max: {e}")))?,
                None => return Err(D::Error::custom("until.max is required")),
            };

            let steps: Vec<Step> = match map_get(map, "steps") {
                Some(v) => serde_yaml::from_value(v.clone())
                    .map_err(|e| D::Error::custom(format!("steps: {e}")))?,
                None => return Err(D::Error::custom("until.steps is required")),
            };

            return Ok(Step::Until { until, max, steps });
        }

        if map_get(map, "if").is_some() {
            let cond: String = serde_yaml::from_value(map_get(map, "if").expect("checked").clone())
                .map_err(|e| D::Error::custom(format!("if: {e}")))?;

            let then: Vec<Step> = match map_get(map, "then") {
                Some(v) => serde_yaml::from_value(v.clone())
                    .map_err(|e| D::Error::custom(format!("then: {e}")))?,
                None => return Err(D::Error::custom("if.then is required")),
            };

            let else_steps: Vec<Step> = match map_get(map, "else") {
                Some(v) => serde_yaml::from_value(v.clone())
                    .map_err(|e| D::Error::custom(format!("else: {e}")))?,
                None => Vec::new(),
            };

            return Ok(Step::If {
                cond,
                then,
                else_steps,
            });
        }

        if map_get(map, "run").is_some() {
            let run: String = serde_yaml::from_value(map_get(map, "run").expect("checked").clone())
                .map_err(|e| D::Error::custom(format!("run: {e}")))?;

            let register: Option<String> = match map_get(map, "register") {
                Some(v) => Some(
                    serde_yaml::from_value(v.clone())
                        .map_err(|e| D::Error::custom(format!("register: {e}")))?,
                ),
                None => None,
            };

            return Ok(Step::Run { run, register });
        }

        Err(D::Error::custom(
            "step must contain one of: send, run, until, if",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_send_only_playbook() {
        let pb = parse_playbook(
            r#"
name: hello
description: say hi
steps:
  - send:
      agent: chat
      message: hello
"#,
        )
        .unwrap();
        assert_eq!(pb.name, "hello");
        assert_eq!(pb.description.as_deref(), Some("say hi"));
        match &pb.steps[0] {
            Step::Send { send } => {
                assert_eq!(send.agent, "chat");
                assert_eq!(send.message, "hello");
            }
            other => panic!("expected send, got {other:?}"),
        }
    }

    #[test]
    fn parse_params_run_until_if() {
        let pb = parse_playbook(
            r#"
name: pr-comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - run: echo hi
    register: COMMENTS
  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: review ${GITHUB_PR_URL}
  - if: test -n "$COMMENTS"
    then:
      - send:
          agent: plan
          message: make a plan
    else:
      - send:
          agent: chat
          message: nothing to do
"#,
        )
        .unwrap();
        assert_eq!(pb.params[0].0, "GITHUB_PR_URL");
        assert_eq!(pb.params[0].1.prompt, "GitHub PR URL");
        assert!(pb.params[0].1.required);
        match &pb.steps[0] {
            Step::Run { run, register } => {
                assert_eq!(run, "echo hi");
                assert_eq!(register.as_deref(), Some("COMMENTS"));
            }
            other => panic!("{other:?}"),
        }
        match &pb.steps[1] {
            Step::Until { until, max, steps } => {
                assert_eq!(until, r#"test "$COMMENTS" = "0""#);
                assert_eq!(*max, 5);
                assert!(matches!(steps[0], Step::Send { .. }));
            }
            other => panic!("{other:?}"),
        }
        match &pb.steps[2] {
            Step::If {
                cond,
                then,
                else_steps,
            } => {
                assert_eq!(cond, r#"test -n "$COMMENTS""#);
                assert_eq!(then.len(), 1);
                assert_eq!(else_steps.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn until_without_max_is_error() {
        let err = parse_playbook(
            r#"
name: bad
steps:
  - until: test 1 -eq 1
    steps:
      - send:
          agent: chat
          message: x
"#,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.to_lowercase().contains("max"),
            "expected max required, got {msg}"
        );
    }
}
