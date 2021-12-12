//! # Pallet IPS
//! Intellectual Property Sets
//!
//! - [`Config`]
//! - [`Call`]
//! - [`Pallet`]
//!
//! ## Overview
//! This pallet demonstrates how to create and manage IP Sets, which are sets of tokenized IP components, or IP Tokens.
//!
//! ### Pallet Functions
//!
//! - `create` - Create a new IP Set
//! - `send` - Transfer IP Set owner account address
//! - `list` - List an IP Set for sale
//! - `buy` - Buy an IP Set
//! - `destroy` - Delete an IP Set and all of its contents

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::unused_unit)]

use frame_support::{
    pallet_prelude::*,
    traits::{Currency as FSCurrency, ExistenceRequirement, Get},
    BoundedVec, Parameter,
};
use frame_system::pallet_prelude::*;
use sp_runtime::traits::{AtLeast32BitUnsigned, CheckedAdd, Member, One};
use sp_std::{convert::TryInto, vec::Vec};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

/// Import the primitives crate
use primitives::IpsInfo;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use ipf::{IpfByOwner, IpfStorage};
    use scale_info::prelude::fmt::Display;
    use scale_info::prelude::format;
    use sp_runtime::traits::StaticLookup;

    #[pallet::config]
    pub trait Config: frame_system::Config + ipf::Config + pallet_assets::Config {
        /// The IPS Pallet Events
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        /// The IPS ID type
        type IpsId: Parameter
            + Member
            + AtLeast32BitUnsigned
            + Default
            + Copy
            + Display
            + IsType<<Self as pallet_assets::Config>::AssetId>;
        /// The maximum size of an IPS's metadata
        type MaxIpsMetadata: Get<u32>;
        /// Currency
        type Currency: FSCurrency<Self::AccountId>;

        type IpsData: IntoIterator + Clone;

        #[pallet::constant]
        type ExistentialDeposit: Get<<Self as pallet_assets::Config>::Balance>;
    }

    pub type BalanceOf<T> =
        <<T as Config>::Currency as FSCurrency<<T as frame_system::Config>::AccountId>>::Balance;

    pub type IpsIndexOf<T> = <T as Config>::IpsId;

    pub type IpsMetadataOf<T> = BoundedVec<u8, <T as Config>::MaxIpsMetadata>;

    pub type IpsInfoOf<T> = IpsInfo<
        <T as frame_system::Config>::AccountId,
        Vec<<T as ipf::Config>::IpfId>,
        IpsMetadataOf<T>,
    >;

    pub type GenesisIps<T> = (
        <T as frame_system::Config>::AccountId, // IPS owner
        Vec<u8>,                                // IPS metadata
        Vec<<T as ipf::Config>::IpfId>,         // IPS data
        Vec<ipf::GenesisIpfData<T>>,            // Vector of IPFs belong to this IPS
    );

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Next available IPS ID.
    #[pallet::storage]
    #[pallet::getter(fn next_ips_id)]
    pub type NextIpsId<T: Config> = StorageValue<_, T::IpsId, ValueQuery>;

    /// Store IPS info
    ///
    /// Return `None` if IPS info not set of removed
    #[pallet::storage]
    #[pallet::getter(fn ips_storage)]
    pub type IpsStorage<T: Config> = StorageMap<_, Blake2_128Concat, T::IpsId, IpsInfoOf<T>>;

    /// IPS existence check by owner and IPS ID
    #[pallet::storage]
    #[pallet::getter(fn ips_by_owner)]
    pub type IpsByOwner<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId, // owner
        Blake2_128Concat,
        T::IpsId,
        (),
    >;

    /// Get IPS price. None means not for sale.
    #[pallet::storage]
    #[pallet::getter(fn ips_prices)]
    pub type IpsPrices<T: Config> =
        StorageMap<_, Blake2_128Concat, T::IpsId, BalanceOf<T>, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(fn deposit_event)]
    // #[pallet::metadata(T::AccountId = "AccountId", T::IpsId = "IpsId")]
    pub enum Event<T: Config> {
        Created(T::AccountId, T::IpsId),
        Sent(T::AccountId, T::AccountId, T::IpsId),
        Listed(T::AccountId, T::IpsId, Option<BalanceOf<T>>),
        Bought(T::AccountId, T::AccountId, T::IpsId, BalanceOf<T>),
        Destroyed(T::AccountId, T::IpsId),
    }

    /// Errors for IPF pallet
    #[pallet::error]
    pub enum Error<T> {
        /// No available IPS ID
        NoAvailableIpsId,
        /// No available IPF ID
        NoAvailableIpfId,
        /// IPF (IpsId, IpfId) not found
        IpfNotFound,
        /// IPS not found
        IpsNotFound,
        /// The operator is not the owner of the IPF and has no permission
        NoPermission,
        /// The IPS is already owned
        AlreadyOwned,
        /// Failed because the Maximum amount of metadata was exceeded
        MaxMetadataExceeded,
        /// List for the same price already listed
        SamePrice,
        /// Buy IPS from their self
        BuyFromSelf,
        /// IPS is not for sale
        NotForSale,
        /// Buy price is too low
        PriceTooLow,
        /// Can not destroy IPS
        CannotDestroyIps,
    }

    /// Dispatch functions
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create IP (Intellectual Property) Set (IPS)
        #[pallet::weight(100_000 + T::DbWeight::get().reads_writes(1, 2))]
        pub fn create_ips(
            owner: OriginFor<T>,
            metadata: Vec<u8>,
            data: Vec<<T as ipf::Config>::IpfId>,
        ) -> DispatchResultWithPostInfo {
            NextIpsId::<T>::try_mutate(|ips_id| -> DispatchResultWithPostInfo {
                let creator = ensure_signed(owner.clone())?;

                let bounded_metadata: BoundedVec<u8, T::MaxIpsMetadata> = metadata
                    .try_into()
                    .map_err(|_| Error::<T>::MaxMetadataExceeded)?;

                let current_id = *ips_id;
                *ips_id = ips_id
                    .checked_add(&One::one())
                    .ok_or(Error::<T>::NoAvailableIpsId)?;

                ensure!(
                    !data.clone().into_iter().any(|ipf_id| {
                        ipf::IpfByOwner::<T>::get(creator.clone(), ipf_id).is_none()
                    }),
                    Error::<T>::NoPermission
                );

                pallet_assets::Pallet::<T>::create(
                    owner.clone(),
                    current_id.into(),
                    T::Lookup::unlookup(creator.clone()),
                    T::ExistentialDeposit::get(),
                )?;

                pallet_assets::Pallet::<T>::set_metadata(
                    owner,
                    current_id.into(),
                    format!("IPO {}", ips_id.clone()).as_bytes().to_vec(),
                    format!("$IPO {}", ips_id.clone()).as_bytes().to_vec(),
                    18,
                )?;

                let ips_account =
                    primitives::utils::multi_account_id::<T, <T as Config>::IpsId>(current_id);

                let info = IpsInfo {
                    owner: ips_account.clone(),
                    metadata: bounded_metadata,
                    data,
                };

                IpsStorage::<T>::insert(current_id, info);
                IpsByOwner::<T>::insert(ips_account.clone(), current_id, ());

                Self::deposit_event(Event::Created(ips_account, current_id));

                Ok(().into())
            })
        }

        /// Transfer IP Set owner account address
        #[pallet::weight(100_000 + T::DbWeight::get().reads_writes(1, 2))]
        pub fn send(from: OriginFor<T>, to: T::AccountId, ips_id: T::IpsId) -> DispatchResult {
            IpsStorage::<T>::try_mutate(ips_id, |ips_info| -> DispatchResult {
                let owner = ensure_signed(from)?;
                let mut info = ips_info.as_mut().ok_or(Error::<T>::IpsNotFound)?;
                ensure!(info.owner == owner, Error::<T>::NoPermission);
                ensure!(owner != to, Error::<T>::AlreadyOwned);

                info.owner = to.clone();

                IpsByOwner::<T>::remove(owner.clone(), ips_id);
                IpsByOwner::<T>::insert(to.clone(), ips_id, ());

                Self::deposit_event(Event::Sent(owner, to, ips_id));

                Ok(())
            })
        }

        /// List a IPS for sale
        /// None to delist the IPS
        #[pallet::weight(100_000 + T::DbWeight::get().reads_writes(1, 2))]
        pub fn list(
            owner: OriginFor<T>,
            ips_id: T::IpsId,
            // ips_index: IpsInfoOf<T>,
            new_price: Option<BalanceOf<T>>,
        ) -> DispatchResult {
            IpsPrices::<T>::try_mutate_exists(ips_id, |price| -> DispatchResult {
                let owner = ensure_signed(owner)?;

                let info = IpsStorage::<T>::get(ips_id).ok_or(Error::<T>::IpsNotFound)?;
                ensure!(info.owner == owner, Error::<T>::NoPermission);
                ensure!(*price != new_price, Error::<T>::SamePrice);

                *price = new_price;

                Self::deposit_event(Event::Listed(owner, ips_id, new_price));

                Ok(())
            })
        }

        /// Allow a user to buy an IPS
        #[pallet::weight(100_000 + T::DbWeight::get().reads_writes(1, 2))]
        pub fn buy(
            buyer: OriginFor<T>,
            ips_id: T::IpsId,
            max_price: BalanceOf<T>,
        ) -> DispatchResult {
            IpsPrices::<T>::try_mutate_exists(ips_id, |price| -> DispatchResult {
                let buyer_signed = ensure_signed(buyer)?;

                let ips = IpsStorage::<T>::get(ips_id)
                    .take()
                    .ok_or(Error::<T>::IpsNotFound)?;

                ensure!(buyer_signed != ips.owner, Error::<T>::BuyFromSelf);

                let price = price.take().ok_or(Error::<T>::NotForSale)?;

                ensure!(max_price >= price, Error::<T>::PriceTooLow);

                IpsStorage::<T>::try_mutate(ips_id, |ips_info| -> DispatchResult {
                    let mut info = ips_info.as_mut().ok_or(Error::<T>::IpsNotFound)?;
                    IpsByOwner::<T>::remove(info.owner.clone(), ips_id);

                    <T as pallet::Config>::Currency::transfer(
                        &buyer_signed,
                        &info.owner,
                        price,
                        ExistenceRequirement::KeepAlive,
                    )?;

                    info.owner = buyer_signed.clone();

                    IpsByOwner::<T>::insert(info.owner.clone(), ips_id, ());

                    info.data.clone().into_iter().for_each(|ipf_id| {
                        IpfStorage::<T>::mutate(ipf_id, |ipf| {
                            IpfByOwner::<T>::swap(
                                ipf.clone().unwrap().owner,
                                ipf_id,
                                info.owner.clone(),
                                ipf_id,
                            );

                            ipf.as_mut()
                                .expect("IPS cannot be created with a non existent IPF")
                                .owner = info.owner.clone();
                        });
                    });

                    Self::deposit_event(Event::Bought(
                        info.owner.clone(),
                        buyer_signed,
                        ips_id,
                        price,
                    ));

                    Ok(())
                })
            })
        }

        /// Delete an IP Set and all of its contents
        #[pallet::weight(100_000 + T::DbWeight::get().reads_writes(1, 2))]
        pub fn destroy(owner: OriginFor<T>, ips_id: T::IpsId) -> DispatchResult {
            IpsStorage::<T>::try_mutate_exists(ips_id, |ips_info| -> DispatchResult {
                let owner = ensure_signed(owner)?;
                let info = ips_info.take().ok_or(Error::<T>::IpsNotFound)?;
                ensure!(info.owner == owner, Error::<T>::NoPermission);

                IpsByOwner::<T>::remove(owner.clone(), ips_id);

                Self::deposit_event(Event::Destroyed(owner, ips_id));

                Ok(())
            })
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<T::BlockNumber> for Pallet<T> {}
}
