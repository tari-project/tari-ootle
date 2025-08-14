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

import React, {
  useState,
  Children,
  cloneElement,
  useCallback,
  useRef,
  useEffect,
  useMemo,
} from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";
import { styled } from "@mui/material/styles";

interface AccordionGroupProps {
  children: React.ReactNode;
}

const StyledButton = styled(Button)(() => ({
  minHeight: "30px",
  boxShadow: "none",
  textTransform: "none",
  fontSize: "0.8rem",
  fontWeight: 500,
  fontFamily: '"AvenirMedium", sans-serif',
  borderRadius: "32px",
  padding: "6px 32px",
  "&:hover": {
    boxShadow: "none",
  },
}));

function AccordionGroup({ children }: AccordionGroupProps) {
  const [expandAllTrigger, setExpandAllTrigger] = useState<number>(0);
  const [collapseAllTrigger, setCollapseAllTrigger] = useState<number>(0);
  const [accordionStates, setAccordionStates] = useState<boolean[]>([]);
  const childrenArray = Children.toArray(children);
  const accordionStateCallbacks = useRef<((expanded: boolean) => void)[]>([]);

  useEffect(() => {
    setAccordionStates(new Array(childrenArray.length).fill(false));
    accordionStateCallbacks.current = new Array(childrenArray.length).fill(
      null
    );
  }, [childrenArray.length]);

  const handleExpandAll = useCallback((event: React.MouseEvent) => {
    event.stopPropagation();
    setExpandAllTrigger((prev) => prev + 1);
  }, []);

  const handleCollapseAll = useCallback((event: React.MouseEvent) => {
    event.stopPropagation();
    setCollapseAllTrigger((prev) => prev + 1);
  }, []);

  const updateAccordionState = useCallback(
    (index: number, expanded: boolean) => {
      setAccordionStates((prev) => {
        const newStates = [...prev];
        newStates[index] = expanded;
        return newStates;
      });
    },
    []
  );

  const allExpanded = accordionStates.every((state) => state);
  const allCollapsed = accordionStates.every((state) => !state);

  const childrenWithProps = useMemo(() => {
    return Children.map(children, (child, index) => {
      if (React.isValidElement(child)) {
        return cloneElement(child, {
          expandAllTrigger,
          collapseAllTrigger,
          onExpandedChange: (expanded: boolean) =>
            updateAccordionState(index, expanded),
        } as any);
      }
      return child;
    });
  }, [children, expandAllTrigger, collapseAllTrigger, updateAccordionState]);

  return (
    <Box>
      <Stack
        direction="row"
        justifyContent={"flex-end"}
        spacing={1}
        sx={{ marginBottom: 2 }}
      >
        <StyledButton
          variant="outlined"
          size="small"
          onClick={handleExpandAll}
          disabled={allExpanded}
        >
          Expand All
        </StyledButton>
        <StyledButton
          variant="outlined"
          size="small"
          onClick={handleCollapseAll}
          disabled={allCollapsed}
        >
          Collapse All
        </StyledButton>
      </Stack>
      {childrenWithProps}
    </Box>
  );
}

export default AccordionGroup;
