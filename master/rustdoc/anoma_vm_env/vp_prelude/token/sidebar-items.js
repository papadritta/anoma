initSidebarItems({"constant":[["BALANCE_STORAGE_KEY","Key segment for a balance key"],["MAX_DECIMAL_PLACES","Maximum decimal places in a token [`Amount`] and [`Change`]."],["SCALE","Decimal scale of token [`Amount`] and [`Change`]."]],"enum":[["AmountParseError",""],["TransferError",""]],"fn":[["balance_key","Obtain a storage key for user’s balance."],["balance_prefix","Obtain a storage key prefix for all users’ balances."],["is_any_token_balance_key","Check if the given storage key is balance key for unspecified token. If it is, returns the owner."],["is_balance_key","Check if the given storage key is balance key for the given token. If it is, returns the owner."],["is_non_owner_balance_key","Check if the given storage key is non-owner’s balance key. If it is, returns the address."]],"struct":[["Amount","Amount in micro units. For different granularity another representation might be more appropriate."],["Transfer","A simple bilateral token transfer"]],"type":[["Change","A change in tokens amount"]]});