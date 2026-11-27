//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import Divider from "@mui/material/Divider";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import FormHelperText from "@mui/material/FormHelperText";
import Switch from "@mui/material/Switch";
import Typography from "@mui/material/Typography";
import { permissionToString, type Permission } from "@tari-project/ootle-ts-bindings";
import React, { useEffect, useState } from "react";
import "./Permissions.css";

interface OptionalPermissionItem {
  id: number;
  name: string;
  checked: boolean;
}

function buildOptionalPermissions(names: string[]): OptionalPermissionItem[] {
  return names.map((name, index) => ({ id: index, name, checked: true }));
}

export default function Permissions(props: {
  requiredPermissions: Permission[];
  optionalPermissions: Permission[];
  setOptionalPermissions: (permissions: boolean[]) => void;
}) {
  const { requiredPermissions, optionalPermissions, setOptionalPermissions } = props;
  const optionalPermissionsKey = JSON.stringify(
    optionalPermissions.map((permission) => permissionToString(permission)),
  );
  const [permissions, setPermissions] = useState<OptionalPermissionItem[]>(() =>
    buildOptionalPermissions(JSON.parse(optionalPermissionsKey)),
  );

  useEffect(() => {
    const nextNames: string[] = JSON.parse(optionalPermissionsKey);
    setPermissions((prev) => {
      if (prev.length === nextNames.length && prev.every((item, index) => item.name === nextNames[index])) {
        return prev;
      }
      return buildOptionalPermissions(nextNames);
    });
  }, [optionalPermissionsKey]);

  const handleChange = (index: number) => {
    setPermissions((prev) =>
      prev.map((item) => {
        if (item.id === index) {
          return {
            ...item,
            checked: !item.checked,
          };
        }
        return item;
      }),
    );
  };

  useEffect(() => {
    setOptionalPermissions(permissions.map((item) => item.checked));
  }, [permissions, setOptionalPermissions]);

  return (
    <>
      <Typography style={{ textAlign: "center", marginBottom: "20px" }}>
        Select what the app is allowed to access:
      </Typography>
      <FormControl component="fieldset" variant="standard" style={{ width: "100%" }}>
        <Divider />
        <FormGroup>
          {requiredPermissions.map((permission, i) => {
            const label = permissionToString(permission);
            return (
              <React.Fragment key={i}>
                <FormControlLabel
                  control={<Switch checked={true} disabled={true} />}
                  label={label}
                  labelPlacement="start"
                  className="permissions-switch"
                />
                <Divider />
              </React.Fragment>
            );
          })}
        </FormGroup>
        <FormGroup>
          {permissions.map(({ checked, name, id }) => (
            <React.Fragment key={id}>
              <FormControlLabel
                control={<Switch checked={checked} onChange={() => handleChange(id)} name={name} value={name} />}
                label={name}
                labelPlacement="start"
                className="permissions-switch"
              />
              <Divider />
            </React.Fragment>
          ))}
        </FormGroup>
        <FormHelperText style={{ marginBottom: "20px", marginTop: "20px" }}>
          You may be sharing sensitive information with this site. Approve or deny access above.
        </FormHelperText>
      </FormControl>
    </>
  );
}
