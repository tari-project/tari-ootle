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

import CloseRoundedIcon from "@mui/icons-material/CloseRounded";
import SearchIcon from "@mui/icons-material/Search";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import TextField from "@mui/material/TextField";
import React, { useCallback, useEffect, useRef, useState } from "react";

interface IFilterItems {
  title: string;
  value: string;
  filterFn: (value: string, row: any) => boolean;
}

interface ISearchProps {
  setPage: (page: number) => void;
  stateObject: any;
  setStateObject: any;
  filterItems: IFilterItems[];
  placeholder: string;
  defaultSearch?: string;
}

// The stateObject being passed to the filter function needs to have an id property for the filter to work

const TransactionFilter = ({
  setPage,
  stateObject,
  setStateObject,
  filterItems,
  placeholder,
  defaultSearch = "id",
}: ISearchProps) => {
  const [formState, setFormState] = useState({ searchValue: "" });
  const [filterBy, setFilterBy] = useState(defaultSearch);
  const [showClearBtn, setShowClearBtn] = useState(false);
  const [initialUpdate, setInitialUpdate] = useState(true);
  const filterInputRef = useRef<any>(null);
  const stateObjectRef = useRef(stateObject);
  const filterItemsRef = useRef(filterItems);
  const searchStateRef = useRef({ searchValue: formState.searchValue, filterBy });
  const prevSearchRef = useRef({ searchValue: formState.searchValue, filterBy });

  useEffect(() => {
    filterItemsRef.current = filterItems;
    searchStateRef.current = { searchValue: formState.searchValue, filterBy };
  }, [filterItems, formState.searchValue, filterBy]);

  const onSelectChange = (event: any) => {
    setFilterBy(event.target.value as string);
  };

  // when search input changes
  const onTextChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    // e.preventDefault();
    setFormState({ ...formState, [e.target.name]: e.target.value });
    setShowClearBtn(true);
  };

  // search function
  const requestSearch = useCallback(
    (searchedVal: string, filter: string, resetPage: boolean) => {
      const currentStateObject = stateObjectRef.current;
      const filterItem = filterItemsRef.current.find((item) => item.value === filter);
      if (!filterItem) {
        return;
      }

      const updatedObject = currentStateObject.map((row: any) => ({
        ...row,
        show: Boolean(filterItem.filterFn(searchedVal, row)),
      }));

      setStateObject(updatedObject);

      if (resetPage) {
        setPage(0);
      }
    },
    [setStateObject, setPage],
  );

  // Re-apply the active filter whenever the source rows update (initial load, tab switch, or refetch)
  useEffect(() => {
    stateObjectRef.current = stateObject;
    const { searchValue, filterBy: activeFilter } = searchStateRef.current;
    requestSearch(searchValue, activeFilter, false);
  }, [stateObject, requestSearch]);

  // when search input changes and formState has been updated
  useEffect(() => {
    if (prevSearchRef.current.searchValue === formState.searchValue && prevSearchRef.current.filterBy === filterBy) {
      return;
    }
    prevSearchRef.current = { searchValue: formState.searchValue, filterBy };
    const timeoutId = setTimeout(() => {
      if (formState.searchValue === "") {
        setShowClearBtn(false);
      }
      requestSearch(formState.searchValue, filterBy, true);
    }, 300); // Debounce search

    return () => clearTimeout(timeoutId);
  }, [formState.searchValue, filterBy, requestSearch]);

  // once selected filter, focus on input

  useEffect(() => {
    if (filterBy !== "" && !initialUpdate) {
      filterInputRef.current.focus();
    } else {
      setInitialUpdate(false);
    }
  }, [filterBy, initialUpdate]);

  // search function when enter is pressed
  const confirmSearch = () => {
    requestSearch(formState.searchValue, filterBy, true);
    if (formState.searchValue !== "") {
      setShowClearBtn(true);
    }
  };

  // clear search
  const cancelSearch = () => {
    setFormState({ ...formState, searchValue: "" });
    setShowClearBtn(false);
    // setFilterBy('');
  };

  return (
    <div className="flex-container">
      <FormControl>
        <InputLabel>Filter By</InputLabel>
        <Select
          value={filterBy}
          label="Filter By"
          renderValue={(selected) => (selected.length === 0 ? <em>Filter By</em> : selected)}
          onChange={onSelectChange}
          size="medium"
          name="filterBy"
          style={{ flexGrow: "1", minWidth: "200px" }}
        >
          <MenuItem disabled value="">
            <em>Filter by</em>
          </MenuItem>
          {filterItems.map((item) => (
            <MenuItem key={item.value} value={item.value}>
              {item.title}
            </MenuItem>
          ))}
        </Select>
      </FormControl>
      <TextField
        value={formState.searchValue}
        name="searchValue"
        onChange={onTextChange}
        style={{ flexGrow: 1 }}
        inputRef={filterInputRef}
        placeholder={placeholder}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            confirmSearch();
          }
        }}
        slotProps={{
          input: {
            startAdornment: (
              <InputAdornment position="start">
                <SearchIcon />
              </InputAdornment>
            ),
            endAdornment: (
              <InputAdornment position="end">
                {showClearBtn && (
                  <IconButton aria-label="Clear search" onClick={cancelSearch}>
                    <CloseRoundedIcon />
                  </IconButton>
                )}
              </InputAdornment>
            ),
          },
        }}
      />
    </div>
  );
};

export default TransactionFilter;
