/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::mem;

use base::generic_channel::GenericSender;
use base::id::PipelineId;
use devtools_traits::DevtoolScriptControlMsg;

use crate::actor::Actor;
use crate::actors::timeline::HighResolutionStamp;

pub struct FramerateActor {
    pub pipeline_id: PipelineId,
    pub script_sender: GenericSender<DevtoolScriptControlMsg>,
    pub is_recording: bool,
    pub ticks: Vec<HighResolutionStamp>,
}

impl Actor for FramerateActor {
    const BASE_NAME: &str = "framerate";
}

impl FramerateActor {
    pub fn add_tick(&mut self, name: String, tick: f64) {
        self.ticks.push(HighResolutionStamp::wrap(tick));

        if self.is_recording {
            let msg = DevtoolScriptControlMsg::RequestAnimationFrame(self.pipeline_id, name);
            self.script_sender.send(msg).unwrap();
        }
    }

    pub fn take_pending_ticks(&mut self) -> Vec<HighResolutionStamp> {
        mem::take(&mut self.ticks)
    }

    pub fn start_recording(&mut self, name: String) {
        if self.is_recording {
            return;
        }

        self.is_recording = true;

        let msg = DevtoolScriptControlMsg::RequestAnimationFrame(self.pipeline_id, name);
        self.script_sender.send(msg).unwrap();
    }

    fn stop_recording(&mut self) {
        if !self.is_recording {
            return;
        }
        self.is_recording = false;
    }
}

impl Drop for FramerateActor {
    fn drop(&mut self) {
        self.stop_recording();
    }
}
