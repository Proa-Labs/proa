// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import * as _ from "marko/html";
export default _._template("WiYwxeF", input => {
  const $scope0_reason = _._scope_reason();
  const $scope0_id = _._scope_id();
  _._for_of(input.indices, i => {
    const $scope1_id = _._scope_id();
    let watching = false;
    _._html(`<button type=button class=lean-watch${_._attr("aria-label", "watch " + i)}>`);
    _._if(() => {
      if (watching) {
        const $scope2_id = _._scope_id();
        _._html("<span class=\"heart heart--on\" aria-hidden=true>\u2665</span>");
        _._scope($scope2_id, {});
        return 0;
      } else {
        const $scope3_id = _._scope_id();
        _._html("<span class=\"heart heart--off\" aria-hidden=true>\u2661</span>");
        _._scope($scope3_id, {});
        return 1;
      }
    }, $scope1_id, "a", 1, 1, /* watching */1, "</button>", 1);
    _._script($scope1_id, "sW8she3");
    _._scope($scope1_id, {
      d: watching
    });
  }, 0, $scope0_id, "a", _._serialize_guard($scope0_reason, /* input.indices */0), _._serialize_guard($scope0_reason, /* input.indices */0), _._serialize_guard($scope0_reason, /* input.indices */0), 0, 1);
  _._serialize_if($scope0_reason, /* input.indices */0) && _._scope($scope0_id, {});
});