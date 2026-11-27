//! Characterization of explicit-only composition (RFC 0024 F07.3, commit 0).
//!
//! Pins every `FinalizedProject` field for fixed explicit layer chains. It was
//! written green before the ordered-composition work and must pass unchanged
//! afterwards: explicit-only input keeps its finalized values, so the cache-key
//! inputs derived from them do not move. Fields are read by dot access so adding
//! a field does not break this file.

use std::path::Path;

use merge::Merge;
use omni_configurations::{
    FinalizedProject, LayerId, LoadedLayer, ProjectLayer,
};
use omni_types::OmniPath;
use serde_json::{Value, json};

fn file(path: &str, doc: &str) -> LoadedLayer {
    LoadedLayer::from_file(
        OmniPath::new(path),
        OmniPath::new(Path::new(path).parent().unwrap()),
        serde_json::from_str::<ProjectLayer>(doc).unwrap(),
    )
}

fn fileless(id: &str, doc: &str) -> LoadedLayer {
    LoadedLayer::fileless(
        LayerId::new(id),
        OmniPath::default(),
        serde_json::from_str::<ProjectLayer>(doc).unwrap(),
    )
}

fn compose(layers: Vec<LoadedLayer>) -> FinalizedProject {
    let mut it = layers.into_iter();
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc.finalize_composed().unwrap()
}

/// Path with `/` separators so expectations are portable.
fn portable(path: &OmniPath) -> String {
    path.path()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

fn projection(p: &FinalizedProject) -> Value {
    let map = |f: &dyn Fn(&str) -> Value, names: Vec<&String>| {
        let mut out = serde_json::Map::new();
        for name in names {
            out.insert(name.clone(), f(name));
        }
        Value::Object(out)
    };
    json!({
        "config": serde_json::to_value(&p.config).unwrap(),
        "file": p.file.as_ref().map(portable),
        "dir": portable(&p.dir),
        "task_cache": map(
            &|n| serde_json::to_value(&p.task_cache[n]).unwrap(),
            p.task_cache.keys().collect(),
        ),
        "task_cache_enabled": map(
            &|n| serde_json::to_value(p.task_cache_enabled[n]).unwrap(),
            p.task_cache_enabled.keys().collect(),
        ),
        "task_cache_enabled_file": map(
            &|n| json!(portable(&p.task_cache_enabled_file[n])),
            p.task_cache_enabled_file.keys().collect(),
        ),
        "task_cache_enabled_origin_files": map(
            &|n| json!(p.task_cache_enabled_origin[n]
                .source
                .file_path()
                .map(|f| portable(f))),
            p.task_cache_enabled_origin.keys().collect(),
        ),
        "task_meta": map(
            &|n| serde_json::to_value(&p.task_meta[n]).unwrap(),
            p.task_meta.keys().collect(),
        ),
        "aliases": format!("{:?}", p.aliases),
    })
}

fn check(name: &str, actual: Value, expected: Value) {
    assert_eq!(
        actual,
        expected,
        "fixture {name}\nactual: {}",
        serde_json::to_string_pretty(&actual).unwrap()
    );
}

fn base_chain() -> FinalizedProject {
    compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":true,"key":{"env":{"append":["A"]}}},
               "env":{"vars":{"X":"1"}},"meta":{"k":1},
               "tasks":{"t":"echo","u":{"exec":"x","cache":{"enabled":false}}}}"#,
        ),
        file(
            "app/p.yaml",
            r#"{"cache":{"enabled":false},"description":"d",
               "tasks":{"t":{"dependencies":["u"],"meta":{"m":2}}}}"#,
        ),
    ])
}

fn fallback_chain() -> FinalizedProject {
    compose(vec![
        file("lib/base.yaml", r#"{"name":"a","tasks":{"t":"echo"}}"#),
        file("app/p.yaml", r#"{"cache":{"remote":false}}"#),
    ])
}

fn mixed_chain() -> FinalizedProject {
    compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":"x","b":{"alias":"t"}}}"#,
        ),
        fileless("applied", r#"{"tasks":{"t":{"cache":{"enabled":true}}}}"#),
    ])
}

#[test]
fn explicit_chain_with_project_and_task_overrides() {
    check(
        "base",
        projection(&base_chain()),
        serde_json::from_str(
            r####"{
  "aliases": "TaskAliases { canonical_by_alias: {} }",
  "config": {
    "base": false,
    "cache": {
      "enabled": false,
      "key": {
        "env": {
          "append": [
            "A"
          ]
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      }
    },
    "dependencies": {
      "append": []
    },
    "description": "d",
    "env": {
      "vars": {
        "X": "1"
      }
    },
    "meta": {
      "k": 1
    },
    "name": "a",
    "redact": {},
    "tasks": {
      "t": {
        "args": {},
        "cache": {
          "key": {
            "env": {
              "append": []
            },
            "files": {
              "append": []
            }
          },
          "output": {
            "files": {
              "append": []
            },
            "logs": true
          }
        },
        "dependencies": [
          "u"
        ],
        "env": {},
        "exec": "echo",
        "meta": {
          "m": 2
        },
        "redact": {},
        "with": {
          "append": []
        }
      },
      "u": {
        "args": {},
        "cache": {
          "enabled": false,
          "key": {
            "env": {
              "append": []
            },
            "files": {
              "append": []
            }
          },
          "output": {
            "files": {
              "append": []
            },
            "logs": true
          }
        },
        "dependencies": {
          "append": []
        },
        "env": {},
        "exec": "x",
        "meta": {},
        "redact": {},
        "with": {
          "append": []
        }
      }
    }
  },
  "dir": "app",
  "file": "app/p.yaml",
  "task_cache": {
    "t": {
      "enabled": false,
      "key": {
        "env": {
          "append": [
            "A"
          ]
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      }
    },
    "u": {
      "enabled": false,
      "key": {
        "env": {
          "append": [
            "A"
          ]
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      }
    }
  },
  "task_cache_enabled": {
    "t": {
      "source": "project-explicit",
      "value": false
    },
    "u": {
      "source": "task-explicit",
      "value": false
    }
  },
  "task_cache_enabled_file": {
    "t": "app/p.yaml",
    "u": "lib/base.yaml"
  },
  "task_cache_enabled_origin_files": {
    "t": "app/p.yaml",
    "u": "lib/base.yaml"
  },
  "task_meta": {
    "t": {
      "k": 1,
      "m": 2
    },
    "u": {
      "k": 1
    }
  }
}"####,
        )
        .unwrap(),
    );
}

#[test]
fn explicit_chain_with_host_fallback() {
    check(
        "fallback",
        projection(&fallback_chain()),
        serde_json::from_str(
            r####"{
  "aliases": "TaskAliases { canonical_by_alias: {} }",
  "config": {
    "base": false,
    "cache": {
      "key": {
        "env": {
          "append": []
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      },
      "remote": false
    },
    "dependencies": {
      "append": []
    },
    "env": {
      "vars": {}
    },
    "meta": {},
    "name": "a",
    "redact": {},
    "tasks": {
      "t": "echo"
    }
  },
  "dir": "app",
  "file": "app/p.yaml",
  "task_cache": {
    "t": {
      "key": {
        "env": {
          "append": []
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      },
      "remote": false
    }
  },
  "task_cache_enabled": {
    "t": {
      "source": "host-fallback",
      "value": true
    }
  },
  "task_cache_enabled_file": {},
  "task_cache_enabled_origin_files": {},
  "task_meta": {
    "t": {}
  }
}"####,
        )
        .unwrap(),
    );
}

#[test]
fn explicit_chain_with_fileless_layer_and_alias() {
    check(
        "mixed",
        projection(&mixed_chain()),
        serde_json::from_str(
            r####"{
  "aliases": "TaskAliases { canonical_by_alias: {\"b\": \"t\"} }",
  "config": {
    "base": false,
    "cache": {
      "enabled": false,
      "key": {
        "env": {
          "append": []
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      }
    },
    "dependencies": {
      "append": []
    },
    "env": {
      "vars": {}
    },
    "meta": {},
    "name": "a",
    "redact": {},
    "tasks": {
      "t": {
        "args": {},
        "cache": {
          "enabled": true,
          "key": {
            "env": {
              "append": []
            },
            "files": {
              "append": []
            }
          },
          "output": {
            "files": {
              "append": []
            },
            "logs": true
          }
        },
        "dependencies": {
          "append": [
            "^t"
          ]
        },
        "env": {},
        "exec": "x",
        "meta": {},
        "redact": {},
        "with": {
          "append": []
        }
      }
    }
  },
  "dir": "",
  "file": null,
  "task_cache": {
    "t": {
      "enabled": true,
      "key": {
        "env": {
          "append": []
        },
        "files": {
          "append": []
        }
      },
      "output": {
        "files": {
          "append": []
        },
        "logs": true
      }
    }
  },
  "task_cache_enabled": {
    "t": {
      "source": "task-explicit",
      "value": true
    }
  },
  "task_cache_enabled_file": {},
  "task_cache_enabled_origin_files": {
    "t": null
  },
  "task_meta": {
    "t": {}
  }
}"####,
        )
        .unwrap(),
    );
}
