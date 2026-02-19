/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use dom_struct::dom_struct;
use script_bindings::str::DOMString;

use crate::dom::bindings::codegen::Bindings::DebuggerResumeEventBinding::ResumeLimitMethods;
use crate::dom::bindings::reflector::{Reflector, reflect_dom_object};
use crate::dom::bindings::root::DomRoot;
use crate::dom::globalscope::GlobalScope;
use crate::script_runtime::CanGc;

#[dom_struct]
pub(crate) struct ResumeLimit {
    reflector_: Reflector,
    type_: DOMString,
}

impl ResumeLimit {
    pub(crate) fn new(global: &GlobalScope, type_: DOMString, can_gc: CanGc) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(Self {
                reflector_: Reflector::new(),
                type_,
            }),
            global,
            can_gc,
        )
    }
}

impl ResumeLimitMethods<crate::DomTypeHolder> for ResumeLimit {
    // check-tidy: no specs after this line
    fn Type_(&self) -> DOMString {
        self.type_.clone()
    }
}
