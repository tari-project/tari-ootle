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

import { useState } from 'react';
import { Link } from 'react-router-dom';
import { renderJson } from '../../utils/helpers';
import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableHead from '@mui/material/TableHead';
import TableRow from '@mui/material/TableRow';
import TablePagination from '@mui/material/TablePagination';
import {
  DataTableCell,
  CodeBlock,
  AccordionIconButton,
} from '../../Components/StyledComponents';
import KeyboardArrowDownIcon from '@mui/icons-material/KeyboardArrowDown';
import KeyboardArrowUpIcon from '@mui/icons-material/KeyboardArrowUp';
import Collapse from '@mui/material/Collapse';
import IconButton from '@mui/material/IconButton';
import { ChevronRight } from '@mui/icons-material';
import { TransactionEntry } from '@tari-project/typescript-bindings';
import {
  useListRecentTransactions,
  useGetTransactionResult,
} from '../../api/hooks/useTransactions';
import FetchStatusCheck from '../../Components/FetchStatusCheck';

function RowData(props: { data: TransactionEntry }) {
  const [open1, setOpen1] = useState(false);
  const [open2, setOpen2] = useState(false);

  const { transaction_id, transaction: tx } = props.data;
  const transaction = tx.V1.body.transaction;

  // Fetch transaction result to get finalized time
  const { data: resultData, isLoading: resultLoading } =
    useGetTransactionResult(transaction_id || '');

  // Extract finalized time from result data
  const finalizedTime =
    resultData?.result &&
    typeof resultData.result === 'object' &&
    'Finalized' in resultData.result
      ? resultData.result.Finalized.finalized_time
      : null;

  return (
    <>
      <TableRow sx={{ borderBottom: 'none' }}>
        <DataTableCell
          sx={{
            borderBottom: 'none',
          }}
        >
          <Link
            to={`/transactions/${transaction_id}`}
            style={{ textDecoration: 'none', color: 'inherit' }}
          >
            {transaction_id}
          </Link>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: 'none', textAlign: 'center' }}>
          {resultLoading ? 'Loading...' : finalizedTime || 'N/A'}
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: 'none', textAlign: 'center' }}>
          <AccordionIconButton
            open={open1}
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen1(!open1);
              setOpen2(false);
            }}
          >
            {open1 ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: 'none', textAlign: 'center' }}>
          <AccordionIconButton
            open={open2}
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen2(!open2);
              setOpen1(false);
            }}
          >
            {open2 ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: 'none', textAlign: 'center' }}>
          <IconButton component={Link} to={`/transactions/${transaction_id}`}>
            <ChevronRight color="inherit" />
          </IconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell
          style={{
            paddingBottom: 0,
            paddingTop: 0,
            borderBottom: 'none',
          }}
          colSpan={5}
        >
          <Collapse in={open1} timeout="auto" unmountOnExit>
            <CodeBlock style={{ marginBottom: '10px' }}>
              {renderJson(transaction.fee_instructions)}
            </CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell style={{ paddingBottom: 0, paddingTop: 0 }} colSpan={5}>
          <Collapse in={open2} timeout="auto" unmountOnExit>
            <CodeBlock style={{ marginBottom: '10px' }}>
              {renderJson(transaction.instructions)}
            </CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

function RecentTransactions() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);

  const { data, isLoading, isError, error } = useListRecentTransactions({
    last_id: null,
    limit: 50,
  });

  const transactions = data?.transactions || [];
  const paginatedTransactions = transactions.slice(
    page * rowsPerPage,
    page * rowsPerPage + rowsPerPage
  );

  const handleChangePage = (
    _event: React.MouseEvent<HTMLButtonElement> | null,
    newPage: number
  ) => {
    setPage(newPage);
  };

  const handleChangeRowsPerPage = (
    event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>
  ) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  return (
    <FetchStatusCheck
      isLoading={isLoading}
      isError={isError}
      errorMessage={
        error ? error.message : 'Error fetching transaction details.'
      }
    >
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell>Transaction Hash</TableCell>
              <TableCell style={{ textAlign: 'center' }}>
                Finalized Time
              </TableCell>
              <TableCell style={{ textAlign: 'center' }}>
                Fee Instructions
              </TableCell>
              <TableCell style={{ textAlign: 'center' }}>
                Instructions
              </TableCell>
              <TableCell style={{ textAlign: 'center' }}>Details</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {paginatedTransactions.map((data, i) => (
              <RowData key={i} data={data} />
            ))}
          </TableBody>
        </Table>
      </TableContainer>
      <TablePagination
        component="div"
        count={transactions.length}
        page={page}
        onPageChange={handleChangePage}
        rowsPerPage={rowsPerPage}
        onRowsPerPageChange={handleChangeRowsPerPage}
        rowsPerPageOptions={[5, 10, 25, 50]}
      />
    </FetchStatusCheck>
  );
}

export default RecentTransactions;
