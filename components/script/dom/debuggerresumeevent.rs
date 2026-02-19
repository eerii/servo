/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::fmt::Debug;

use dom_struct::dom_struct;
use script_bindings::root::Dom;

use crate::dom::bindings::codegen::Bindings::DebuggerResumeEventBinding::DebuggerResumeEventMethods;
use crate::dom::bindings::codegen::Bindings::EventBinding::Event_Binding::EventMethods;
use crate::dom::bindings::reflector::reflect_dom_object;
use crate::dom::bindings::root::DomRoot;
use crate::dom::event::Event;
use crate::dom::types::{GlobalScope, ResumeLimit};
use crate::script_runtime::CanGc;

#[dom_struct]
/// Event for Rust → JS calls in [`crate::dom::DebuggerGlobalScope`].
pub(crate) struct DebuggerResumeEvent {
    event: Event,
    resume_limit: Option<Dom<ResumeLimit>>,
}

impl DebuggerResumeEvent {
    pub(crate) fn new(
        debugger_global: &GlobalScope,
        resume_limit: Option<&ResumeLimit>,
        can_gc: CanGc,
    ) -> DomRoot<Self> {
        let result = Box::new(Self {
            event: Event::new_inherited(),
            resume_limit: resume_limit.map(Dom::from_ref),
        });
        let result = reflect_dom_object(result, debugger_global, can_gc);
        result.event.init_event("resume".into(), false, false);

        result
    }
}

impl DebuggerResumeEventMethods<crate::DomTypeHolder> for DebuggerResumeEvent {
    // check-tidy: no specs after this line
    fn IsTrusted(&self) -> bool {
        self.event.IsTrusted()
    }

    fn GetResumeLimit(
        &self,
    ) -> Option<DomRoot<<crate::DomTypeHolder as script_bindings::DomTypes>::ResumeLimit>> {
        self.resume_limit
            .as_ref()
            .map(|resume_limit| DomRoot::from_ref(&**resume_limit))
    }
}

impl Debug for DebuggerResumeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebuggerResumeEvent").finish()
    }
}
